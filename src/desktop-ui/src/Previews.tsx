import { useCallback, useEffect, useState } from "react";
import {
  Check, Copy, Eye, ExternalLink, Globe, Trash2, RefreshCw, FileText, Server,
} from "lucide-react";
import {
  PreviewsResponseSchema,
  type PreviewSnapshot,
  type PreviewsResponse,
} from "@va/client";
import { useI18n } from "@va/i18n";

import { EmptyBlock, PageHeader, PageShell, StatusBanner } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { apiFetch, openExternalUrl, API_BASE } from "./lib/api";

const POLL_INTERVAL_MS = 5000;

export function Previews() {
  const { t } = useI18n();
  const [data, setData] = useState<PreviewsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const fetchPreviews = useCallback(async () => {
    setError("");
    try {
      const res = await apiFetch(`/api/previews`);
      if (!res.ok) throw new Error(await res.text());
      setData(PreviewsResponseSchema.parse(await res.json()));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchPreviews();
    const id = setInterval(fetchPreviews, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchPreviews]);

  const closePreview = async (slug: string) => {
    try {
      const res = await apiFetch(`/api/previews/${encodeURIComponent(slug)}`, {
        method: "DELETE",
      });
      if (!res.ok && res.status !== 404) throw new Error(await res.text());
      fetchPreviews();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const tunnelUrl = data?.tunnel_url ?? null;
  const localBase = API_BASE.replace(/\/va\/?$/, "");

  return (
    <PageShell>
      <PageHeader
        icon={<Eye className="w-4 h-4 text-primary" />}
        title={t("Previews")}
        actions={(
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            onClick={fetchPreviews}
            title={t("Refresh")}
          >
            <RefreshCw
              className={`w-3.5 h-3.5 text-muted-foreground ${loading ? "animate-spin" : ""}`}
            />
          </Button>
        )}
      />

      {error && (
        <StatusBanner>{error}</StatusBanner>
      )}

      <div className="rounded-md border border-border bg-card overflow-hidden">
        {data?.previews.map((p, i) => (
          <PreviewRow
            key={p.slug}
            preview={p}
            tunnelUrl={tunnelUrl}
            localBase={localBase}
            isFirst={i === 0}
            onClose={() => closePreview(p.slug)}
          />
        ))}
        {(!data || data.previews.length === 0) && !loading && (
          <EmptyBlock>
            {t("No active previews. Ask your coding agent to run preview or md_preview.")}
          </EmptyBlock>
        )}
      </div>
    </PageShell>
  );
}

interface PreviewRowProps {
  preview: PreviewSnapshot;
  tunnelUrl: string | null;
  localBase: string;
  isFirst: boolean;
  onClose: () => void;
}

type CopyStatus = "idle" | "copied" | "failed";

function PreviewRow({ preview, tunnelUrl, localBase, isFirst, onClose }: PreviewRowProps) {
  const { locale, t } = useI18n();
  const [copyStatus, setCopyStatus] = useState<CopyStatus>("idle");
  const ownerPath = `/va/preview/u/${encodeURIComponent(preview.slug)}`;
  const sharePath = preview.share_id
    && preview.share_code
    && preview.share_expires_at_ms
    && preview.share_expires_at_ms > Date.now()
    ? `/va/preview/s/${encodeURIComponent(preview.share_id)}`
    : null;

  const localOwnerUrl = `${localBase}${ownerPath}`;
  const tunnelOwnerUrl = tunnelUrl ? `${tunnelUrl}${ownerPath}` : null;
  const tunnelShareUrl = tunnelUrl && sharePath ? `${tunnelUrl}${sharePath}` : null;
  const shareMessage = tunnelShareUrl && preview.share_code && preview.share_expires_at_ms
    ? t("Access code: {{code}} (reusable by multiple viewers until expiry)\nVibeAround Preview: {{title}}\n{{url}}\nExpires: {{expires}}", {
        title: preview.title,
        url: tunnelShareUrl,
        code: preview.share_code,
        expires: new Date(preview.share_expires_at_ms).toLocaleString(locale, {
          timeZoneName: "short",
        }),
      })
    : null;
  const displayCode = preview.share_code?.replace(/^(\d{3})(\d{3})$/, "$1 $2");

  const Icon = preview.kind === "server" ? Server : FileText;

  return (
    <div className={`px-3 py-2 ${isFirst ? "" : "border-t border-border"}`}>
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-start gap-3 min-w-0 flex-1">
          <Icon className="w-4 h-4 text-muted-foreground shrink-0 mt-0.5" />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-[13px] font-semibold truncate">{preview.title}</span>
              <Badge
                className={`text-[10px] ${
                  preview.kind === "server"
                    ? "bg-emerald-500/10 text-emerald-600"
                    : "bg-blue-500/10 text-blue-600"
                }`}
              >
                {preview.kind}
              </Badge>
              {preview.port != null && (
                <Badge variant="secondary" className="text-[10px] font-mono">
                  :{preview.port}
                </Badge>
              )}
            </div>
            <div
              className="text-xs text-muted-foreground font-mono truncate mt-0.5"
              title={preview.id}
            >
              {preview.id}
            </div>
          </div>
        </div>

        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onClose}
          className="shrink-0 hover:bg-destructive/10"
          title={preview.kind === "server" ? t("Close (kills dev server)") : t("Close")}
        >
          <Trash2 className="w-3.5 h-3.5 text-muted-foreground hover:text-destructive" />
        </Button>
      </div>

      <div className="flex items-center gap-1.5 mt-2 flex-wrap">
        <UrlButton
          label={t("Local")}
          url={localOwnerUrl}
          icon={<ExternalLink className="w-3 h-3" />}
          openUrl={openExternalUrl}
        />
        <UrlButton
          label={t("Tunnel · owner")}
          url={tunnelOwnerUrl}
          icon={<Globe className="w-3 h-3" />}
          openUrl={openExternalUrl}
          disabledReason={tunnelOwnerUrl ? null : t("Tunnel not running")}
        />
        <UrlButton
          label={t("Tunnel · share")}
          url={tunnelShareUrl}
          icon={<Globe className="w-3 h-3" />}
          openUrl={openExternalUrl}
          disabledReason={
            !tunnelUrl
              ? t("Tunnel not running")
              : !sharePath
                ? t("Access code expired")
                : null
          }
        />
        {shareMessage && (
          <>
            <span className="px-1 text-[11px] text-muted-foreground">
              {t("Access code")}: <code className="font-mono text-foreground">{displayCode}</code>
            </span>
            <Button
              type="button"
              variant="secondary"
              size="xs"
              className="h-7 text-[11px] bg-primary/10 text-primary hover:bg-primary/20"
              title={copyStatus === "copied"
                ? t("Copied")
                : copyStatus === "failed"
                  ? t("Copy failed")
                  : t("Copy share message")}
              onClick={() => {
                void (async () => {
                  try {
                    await navigator.clipboard.writeText(shareMessage);
                    setCopyStatus("copied");
                  } catch {
                    setCopyStatus("failed");
                  } finally {
                    window.setTimeout(() => setCopyStatus("idle"), 1600);
                  }
                })();
              }}
            >
              {copyStatus === "copied"
                ? <Check className="w-3 h-3" />
                : <Copy className="w-3 h-3" />}
              {copyStatus === "copied"
                ? t("Copied")
                : copyStatus === "failed"
                  ? t("Copy failed")
                  : t("Copy share message")}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}

interface UrlButtonProps {
  label: string;
  url: string | null;
  icon: React.ReactNode;
  openUrl: (url: string) => Promise<void>;
  disabledReason?: string | null;
}

function UrlButton({ label, url, icon, openUrl, disabledReason }: UrlButtonProps) {
  const { t } = useI18n();
  const disabled = !url || !!disabledReason;
  const onClick = () => {
    if (!url) return;
    void openUrl(url);
  };
  return (
    <Button
      type="button"
      variant="secondary"
      size="xs"
      disabled={disabled}
      onClick={onClick}
      title={disabled ? disabledReason ?? t("Unavailable") : url ?? ""}
      className="h-7 text-[11px] bg-primary/10 text-primary hover:bg-primary/20 disabled:bg-muted disabled:text-muted-foreground/50"
    >
      {icon}
      {label}
    </Button>
  );
}
