import { useEffect, useMemo, useState } from "react";
import { Check, Monitor, RefreshCw } from "lucide-react";
import { useI18n } from "@va/i18n";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  listWindowsStartApps,
  type WindowsStartAppEntry,
} from "./api";

interface Props {
  open: boolean;
  agentId: string;
  agentName: string;
  selectedAppId: string;
  onOpenChange: (open: boolean) => void;
  onSelect: (app: WindowsStartAppEntry) => void;
}

export function WindowsStartAppPickerDialog({
  open,
  agentId,
  agentName,
  selectedAppId,
  onOpenChange,
  onSelect,
}: Props) {
  const { t } = useI18n();
  const [apps, setApps] = useState<WindowsStartAppEntry[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setQuery("");
    setLoading(true);
    setError(null);
    void listWindowsStartApps(agentId)
      .then((entries) => {
        if (!cancelled) setApps(entries);
      })
      .catch((loadError) => {
        if (!cancelled) {
          setApps([]);
          setError(
            loadError instanceof Error ? loadError.message : String(loadError),
          );
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [agentId, open]);

  const visibleApps = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return apps;
    return apps.filter(
      (app) =>
        app.name.toLowerCase().includes(needle) ||
        app.appId.toLowerCase().includes(needle),
    );
  }, [apps, query]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] flex-col overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {t("{{agent}} installed apps", { agent: agentName })}
          </DialogTitle>
          <DialogDescription>
            {t("Select an app from the Windows Start menu.")}
          </DialogDescription>
        </DialogHeader>

        <Input
          value={query}
          disabled={loading}
          placeholder={t("Search installed apps")}
          onChange={(event) => setQuery(event.target.value)}
        />

        <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto pr-1">
          {loading ? (
            <div className="px-1 py-4 text-center text-xs text-muted-foreground">
              <RefreshCw className="mr-1.5 inline h-3.5 w-3.5 animate-spin align-[-2px]" />
              {t("Loading installed apps…")}
            </div>
          ) : error ? (
            <div className="px-1 py-3 text-xs text-destructive">{error}</div>
          ) : visibleApps.length ? (
            visibleApps.map((app) => {
              const selected = app.appId === selectedAppId;
              return (
                <button
                  key={app.appId}
                  type="button"
                  className={`flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left transition-colors ${
                    selected
                      ? "border-primary bg-primary/10"
                      : "border-border bg-card hover:border-primary/40 hover:bg-accent/35"
                  }`}
                  onClick={() => onSelect(app)}
                >
                  <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                    <Monitor className="h-4 w-4" />
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm font-medium">
                    {app.name}
                  </span>
                  {app.recommended && (
                    <span className="shrink-0 rounded border border-primary/25 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                      {t("Recommended")}
                    </span>
                  )}
                  {selected && (
                    <Check className="h-4 w-4 shrink-0 text-primary" />
                  )}
                </button>
              );
            })
          ) : (
            <div className="px-1 py-4 text-center text-xs text-muted-foreground">
              {t("No matching installed apps found")}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
