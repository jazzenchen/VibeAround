import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  AlignLeft,
  Check,
  Copy,
  Radio,
  Trash2,
  Wifi,
  WifiOff,
  WrapText,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import { authedWsUrl } from "@/lib/api";

const MAX_RECORDS = 200;

type BridgeRecordPhase =
  | "start"
  | "bridgeRequest"
  | "serverResponse"
  | "bridgeResponse"
  | "serviceSide"
  | "search"
  | "error";

interface RecordedPayload {
  byteLength: number;
  truncated: boolean;
  text: string;
  json?: unknown;
}

interface BridgeRecordMetadata {
  profileId: string;
  routeScope?: string;
  manualScope?: string;
  targetApiType: string;
  clientProtocol: string;
  upstreamProtocol?: string;
  upstreamUrl?: string;
  stream?: boolean;
  model?: string;
  passthrough: boolean;
}

interface BridgeRecordEvent {
  recordId: number;
  requestId: string;
  phase: BridgeRecordPhase;
  timestampMs: number;
  metadata?: BridgeRecordMetadata;
  originalRequest?: RecordedPayload;
  bridgeRequest?: RecordedPayload;
  serverResponse?: RecordedPayload;
  bridgeResponse?: RecordedPayload;
  serviceSide?: RecordedPayload;
  search?: RecordedPayload;
  error?: string;
  status?: number;
}

interface BridgeRecordEntry {
  recordId: number;
  requestId: string;
  timestampMs: number;
  updatedAtMs: number;
  metadata?: BridgeRecordMetadata;
  originalRequest?: RecordedPayload;
  bridgeRequest?: RecordedPayload;
  serverResponse?: RecordedPayload;
  bridgeResponse?: RecordedPayload;
  serviceSidePayloads: RecordedPayload[];
  searchPayloads: RecordedPayload[];
  serverStatus?: number;
  bridgeStatus?: number;
  errors: string[];
  phases: Set<BridgeRecordPhase>;
}

type PayloadTab =
  | "originalRequest"
  | "bridgeRequest"
  | "serverResponse"
  | "bridgeResponse"
  | "serviceSide"
  | "search";

export function BridgeRecordPopup({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useI18n();
  const [records, setRecords] = useState<BridgeRecordEntry[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!open) {
      setRecords([]);
      setSelectedId(null);
      setConnected(false);
      setError(null);
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
        wsRef.current = null;
      }
      return;
    }

    let disposed = false;
    const connect = async () => {
      try {
        const url = await authedWsUrl("/ws/bridge-recording");
        if (disposed) return;
        const ws = new WebSocket(url);
        wsRef.current = ws;
        ws.onopen = () => {
          setConnected(true);
          setError(null);
        };
        ws.onmessage = (event) => {
          try {
            const frame = JSON.parse(event.data) as BridgeRecordEvent;
            setRecords((current) => mergeRecordEvent(current, frame));
            setSelectedId((current) => current ?? frame.recordId);
          } catch {
            // Ignore malformed frames from stale or interrupted sockets.
          }
        };
        ws.onclose = () => {
          setConnected(false);
          wsRef.current = null;
          if (!disposed) {
            reconnectTimerRef.current = setTimeout(() => void connect(), 1000);
          }
        };
        ws.onerror = () => {
          setError(t("WebSocket error"));
        };
      } catch (e) {
        setConnected(false);
        setError(e instanceof Error ? e.message : t("WebSocket error"));
      }
    };

    void connect();
    return () => {
      disposed = true;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [open, t]);

  const selected = useMemo(
    () => records.find((record) => record.recordId === selectedId) ?? records[0],
    [records, selectedId],
  );

  const dialogSize = {
    width: "min(1680px, calc(100vw - 8rem))",
    maxWidth: "calc(100vw - 8rem)",
    height: "calc(100vh - 8rem)",
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex grid-rows-none flex-col gap-0 overflow-hidden p-0"
        style={dialogSize}
        onInteractOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => event.preventDefault()}
      >
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4">
          <div className="flex items-center justify-between gap-4 pr-8">
            <div className="min-w-0">
              <DialogTitle className="flex items-center gap-2 text-base">
                <Radio className="h-4 w-4 text-primary" />
                {t("Bridge recorder")}
              </DialogTitle>
              {error && (
                <DialogDescription className="mt-1 truncate text-xs">
                  {error}
                </DialogDescription>
              )}
            </div>
            <div
              className={cn(
                "flex h-7 items-center gap-1.5 rounded-md border px-2 text-xs",
                connected
                  ? "border-emerald-500/30 text-emerald-600"
                  : "border-border text-muted-foreground",
              )}
            >
              {connected ? (
                <Wifi className="h-3.5 w-3.5" />
              ) : (
                <WifiOff className="h-3.5 w-3.5" />
              )}
              {records.length}
            </div>
          </div>
        </DialogHeader>

        <div className="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)]">
          <aside className="flex min-h-0 flex-col border-r border-border bg-muted/15">
            <div className="flex h-10 shrink-0 items-center justify-between border-b border-border px-3">
              <span className="text-xs font-medium text-muted-foreground">
                {t("Requests")}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={t("Clear")}
                title={t("Clear")}
                onClick={() => {
                  setRecords([]);
                  setSelectedId(null);
                }}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]">
              {records.length === 0 ? (
                <div className="px-3 py-8 text-center text-xs text-muted-foreground">
                  {t("No records")}
                </div>
              ) : (
                records.map((record) => {
                  const isSelected = selected?.recordId === record.recordId;
                  return (
                    <button
                      key={record.recordId}
                      type="button"
                      aria-current={isSelected ? "true" : undefined}
                      className={cn(
                        "flex w-full min-w-0 flex-col gap-1 border-b px-3 py-2 text-left transition-colors",
                        isSelected
                          ? "border-primary/20 bg-primary/10 shadow-[inset_3px_0_0_hsl(var(--primary))]"
                          : "border-border/70 hover:bg-background/60",
                      )}
                      onClick={() => setSelectedId(record.recordId)}
                    >
                      <span className="flex min-w-0 items-center justify-between gap-2">
                        <span className="min-w-0 truncate text-xs font-medium">
                          {record.metadata?.profileId ??
                            record.requestId.slice(0, 8)}
                        </span>
                        <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                          {formatTime(record.timestampMs)}
                        </span>
                      </span>
                      <span className="flex min-w-0 items-center gap-1 text-[11px] text-muted-foreground">
                        <span className="truncate">
                          {record.metadata?.clientProtocol ?? t("client")}
                        </span>
                        <span>-&gt;</span>
                        <span className="truncate">
                          {record.metadata?.upstreamProtocol ?? t("upstream")}
                        </span>
                        {record.metadata?.stream && <span>SSE</span>}
                      </span>
                      <span className="flex items-center gap-1">
                        {payloadPhasesForRecord(record).map((phase) => (
                          <span
                            key={phase}
                            className={cn(
                              "h-1.5 w-1.5 rounded-full",
                              record.phases.has(phase)
                                ? "bg-primary"
                                : "bg-muted-foreground/25",
                            )}
                          />
                        ))}
                        {record.errors.length > 0 && (
                          <AlertCircle
                            className="ml-1 h-3 w-3 text-destructive"
                          />
                        )}
                      </span>
                    </button>
                  );
                })
              )}
            </div>
          </aside>

          <main className="flex min-h-0 min-w-0 flex-col">
            {selected ? (
              <RecordDetails record={selected} />
            ) : (
              <div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">
                {t("No record selected")}
              </div>
            )}
          </main>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function RecordDetails({ record }: { record: BridgeRecordEntry }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<PayloadTab>("originalRequest");
  const [wrapJson, setWrapJson] = useState(false);
  const metadata = record.metadata;
  const scopeLabel = metadata?.manualScope ?? metadata?.routeScope;
  const hasStatuses =
    record.serverStatus !== undefined || record.bridgeStatus !== undefined;
  const serviceSidePayload = useMemo(
    () => serviceSidePayloadForRecord(record),
    [record],
  );
  const searchPayload = useMemo(() => searchPayloadForRecord(record), [record]);
  const visibleTabs = useMemo(
    () => [
      payloadTabs[0],
      ...(serviceSidePayload ? (["serviceSide"] as const) : []),
      payloadTabs[1],
      ...(searchPayload ? (["search"] as const) : []),
      ...payloadTabs.slice(2),
    ],
    [searchPayload, serviceSidePayload],
  );

  useEffect(() => {
    if (!payloadForTab(record, tab, serviceSidePayload, searchPayload)) {
      const first = visibleTabs.find((candidate) =>
        payloadForTab(record, candidate, serviceSidePayload, searchPayload),
      );
      if (first) setTab(first);
    }
  }, [record, searchPayload, serviceSidePayload, tab, visibleTabs]);

  return (
    <>
      <div className="shrink-0 border-b border-border px-4 py-3">
        <div className="flex min-w-0 flex-col gap-2">
          <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
            <div className="min-w-[240px] flex-1 truncate font-mono text-xs text-foreground">
              {record.requestId}
            </div>
            {hasStatuses && (
              <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                {record.serverStatus !== undefined && (
                  <StatusPill status={record.serverStatus}>
                    {t("server {{status}}", { status: record.serverStatus })}
                  </StatusPill>
                )}
                {record.bridgeStatus !== undefined && (
                  <StatusPill status={record.bridgeStatus}>
                    {t("bridge {{status}}", { status: record.bridgeStatus })}
                  </StatusPill>
                )}
              </div>
            )}
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <MetaPill mono>
              <span>{metadata?.clientProtocol ?? t("client")}</span>
              <span className="text-muted-foreground/60">-&gt;</span>
              <span>{metadata?.upstreamProtocol ?? t("upstream")}</span>
            </MetaPill>
            <MetaPill>{metadata?.targetApiType ?? t("Unknown target")}</MetaPill>
            {metadata?.model && <MetaPill mono>{metadata.model}</MetaPill>}
            {metadata?.stream && <MetaPill>SSE</MetaPill>}
            {scopeLabel && <MetaPill mono>{scopeLabel}</MetaPill>}
            {metadata?.passthrough && <MetaPill>passthrough</MetaPill>}
          </div>
          {metadata?.upstreamUrl && (
            <div className="flex min-w-0 items-center gap-2 rounded-md border border-border/70 bg-muted/20 px-2 py-1.5">
              <span className="shrink-0 text-[10px] font-medium uppercase text-muted-foreground">
                URL
              </span>
              <span className="min-w-0 truncate font-mono text-[11px] text-foreground/80">
                {metadata.upstreamUrl}
              </span>
            </div>
          )}
        </div>
        {record.errors.length > 0 && (
          <div className="mt-2 rounded-md border border-destructive/25 bg-destructive/5 px-2 py-1.5 text-xs text-destructive">
            {record.errors.join(" · ")}
          </div>
        )}
      </div>
      <Tabs
        value={tab}
        onValueChange={(value) => setTab(value as PayloadTab)}
        className="min-h-0 flex-1 gap-0"
      >
        <div className="mx-4 mt-3 flex min-h-8 shrink-0 items-center gap-3">
          <TabsList className="h-8 w-fit rounded-md">
            {visibleTabs.map((value) => (
              <TabsTrigger key={value} value={value} className="h-7 text-xs">
                {payloadTabLabel(t, value)}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
        {visibleTabs.map((value) => (
          <TabsContent
            key={value}
            value={value}
            className="mt-0 min-h-0 overflow-hidden px-4 pb-4 pt-3"
          >
            <PayloadViewer
              payload={payloadForTab(
                record,
                value,
                serviceSidePayload,
                searchPayload,
              )}
              wrap={wrapJson}
              onWrapChange={setWrapJson}
            />
          </TabsContent>
        ))}
      </Tabs>
    </>
  );
}

function PayloadViewer({
  payload,
  wrap,
  onWrapChange,
}: {
  payload?: RecordedPayload;
  wrap: boolean;
  onWrapChange: (value: boolean) => void;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    setCopied(false);
    return () => {
      if (copiedTimerRef.current) {
        clearTimeout(copiedTimerRef.current);
        copiedTimerRef.current = null;
      }
    };
  }, [payload]);

  if (!payload) {
    return (
      <div className="flex h-full items-center justify-center rounded-md border border-border bg-muted/20 text-xs text-muted-foreground">
        {t("Empty")}
      </div>
    );
  }
  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-md border border-border bg-background">
      <div className="flex h-8 shrink-0 items-center justify-between border-b border-border px-3 text-[11px] text-muted-foreground">
        <span>{formatBytes(payload.byteLength)}</span>
        <span className="flex items-center gap-2">
          {payload.truncated && <span>{t("Truncated")}</span>}
          <PayloadActions
            wrap={wrap}
            copied={copied}
            onWrapChange={onWrapChange}
            onCopy={() => {
              void copyPayloadToClipboard(payload, () => {
                setCopied(true);
                if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
                copiedTimerRef.current = setTimeout(() => {
                  setCopied(false);
                  copiedTimerRef.current = null;
                }, 1400);
              });
            }}
          />
        </span>
      </div>
      <pre
        className={cn(
          "min-h-0 flex-1 overflow-auto p-3 font-mono text-[11px] leading-5 text-foreground [font-variant-ligatures:none]",
          wrap
            ? "whitespace-pre-wrap break-words"
            : "whitespace-pre break-normal",
        )}
      >
        {payloadText(payload)}
      </pre>
    </div>
  );
}

function PayloadActions({
  wrap,
  copied,
  onWrapChange,
  onCopy,
}: {
  wrap: boolean;
  copied: boolean;
  onWrapChange: (value: boolean) => void;
  onCopy: () => void;
}) {
  const { t } = useI18n();
  const actionClass =
    "flex size-6 items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-background hover:text-foreground";
  return (
    <span
      className="inline-flex h-8 items-center gap-0.5"
      role="group"
      aria-label={t("Payload actions")}
    >
      <button
        type="button"
        className={cn(
          actionClass,
          !wrap && "bg-background text-foreground shadow-sm",
        )}
        title={t("No wrap")}
        aria-label={t("No wrap")}
        aria-pressed={!wrap}
        onClick={() => onWrapChange(false)}
      >
        <AlignLeft className="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        className={cn(
          actionClass,
          wrap && "bg-background text-foreground shadow-sm",
        )}
        title={t("Wrap")}
        aria-label={t("Wrap")}
        aria-pressed={wrap}
        onClick={() => onWrapChange(true)}
      >
        <WrapText className="h-3.5 w-3.5" />
      </button>
      <span className="mx-0.5 h-4 w-px bg-border" aria-hidden="true" />
      <button
        type="button"
        className={cn(
          actionClass,
          copied && "bg-background text-emerald-600 shadow-sm hover:text-emerald-600",
        )}
        title={copied ? t("Copied") : t("Copy")}
        aria-label={copied ? t("Copied") : t("Copy")}
        onClick={onCopy}
      >
        {copied ? (
          <Check className="h-3.5 w-3.5" />
        ) : (
          <Copy className="h-3.5 w-3.5" />
        )}
      </button>
    </span>
  );
}

const payloadTabs: PayloadTab[] = [
  "originalRequest",
  "bridgeRequest",
  "serverResponse",
  "bridgeResponse",
];

const payloadPhases: BridgeRecordPhase[] = [
  "start",
  "serviceSide",
  "bridgeRequest",
  "search",
  "serverResponse",
  "bridgeResponse",
];

function payloadPhasesForRecord(record: BridgeRecordEntry) {
  return payloadPhases.filter((phase) => {
    if (phase === "serviceSide") return record.serviceSidePayloads.length > 0;
    if (phase === "search") return record.searchPayloads.length > 0;
    return true;
  });
}

function serviceSidePayloadForRecord(
  record: BridgeRecordEntry,
): RecordedPayload | undefined {
  if (record.serviceSidePayloads.length === 0) return undefined;
  const json = record.serviceSidePayloads.map(
    (payload) => payload.json ?? parseJson(payload.text) ?? payload.text,
  );
  const text = JSON.stringify(json, null, 2);
  return {
    byteLength: new TextEncoder().encode(text).length,
    truncated: record.serviceSidePayloads.some((payload) => payload.truncated),
    text,
    json,
  };
}

function searchPayloadForRecord(record: BridgeRecordEntry): RecordedPayload | undefined {
  if (record.searchPayloads.length === 0) return undefined;
  const json = record.searchPayloads.map((payload, index) =>
    normalizeSearchTracePayload(payload, index + 1),
  );
  const text = JSON.stringify(json, null, 2);
  return {
    byteLength: new TextEncoder().encode(text).length,
    truncated: record.searchPayloads.some((payload) => payload.truncated),
    text,
    json,
  };
}

function normalizeSearchTracePayload(payload: RecordedPayload, index: number) {
  const raw = payload.json ?? parseJson(payload.text) ?? payload.text;
  const trace = asRecord(raw);
  if (!trace) return { index, payload: raw };

  const request = trace.request;
  const response = asRecord(trace.response);
  const normalized: Record<string, unknown> = { index };
  copyIfPresent(normalized, trace, "round");
  copyIfPresent(normalized, trace, "toolCallId");
  if (request !== undefined) normalized.request = request;

  if (response) {
    normalized.response = {
      provider: response.provider,
      query: response.query,
      sources: searchSourcesFromResponse(response),
    };
  } else if (trace.error !== undefined) {
    normalized.error = trace.error;
  } else {
    normalized.payload = raw;
  }

  return normalized;
}

function searchSourcesFromResponse(response: Record<string, unknown>) {
  const providerNames = splitSearchSources(response.provider);
  const citations = stringArray(response.citations);
  const results = Array.isArray(response.results)
    ? response.results.filter(isRecord)
    : [];
  const sources = new Map<string, Record<string, unknown>[]>();

  for (const name of providerNames) sources.set(name, []);
  for (const result of results) {
    const source =
      stringField(result.source) ?? stringField(result.provider) ?? "unknown";
    sources.set(source, [...(sources.get(source) ?? []), result]);
  }

  if (sources.size === 0) {
    const fallback = stringField(response.provider) ?? "unknown";
    sources.set(fallback, []);
  }

  return Array.from(sources.entries()).map(([source, sourceResults]) => {
    const sourceUrls = new Set(
      sourceResults
        .map((result) => stringField(result.url))
        .filter((url): url is string => !!url),
    );
    const sourceCitations = citations.filter((url) => sourceUrls.has(url));
    return {
      source,
      resultCount: sourceResults.length,
      citations: sourceCitations.length > 0 ? sourceCitations : undefined,
      results: sourceResults,
    };
  });
}

function splitSearchSources(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) => (typeof item === "string" ? item.trim() : ""))
      .filter(Boolean);
  }
  if (typeof value !== "string") return [];
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function copyIfPresent(
  target: Record<string, unknown>,
  source: Record<string, unknown>,
  key: string,
) {
  if (source[key] !== undefined) target[key] = source[key];
}

function parseJson(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return undefined;
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!asRecord(value);
}

function stringField(value: unknown): string | undefined {
  const trimmed = typeof value === "string" ? value.trim() : "";
  return trimmed || undefined;
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => (typeof item === "string" ? item.trim() : ""))
    .filter(Boolean);
}

function payloadForTab(
  record: BridgeRecordEntry,
  tab: PayloadTab,
  serviceSidePayload?: RecordedPayload,
  searchPayload?: RecordedPayload,
) {
  if (tab === "serviceSide") return serviceSidePayload;
  if (tab === "search") return searchPayload;
  return record[tab];
}

function payloadTabLabel(t: (value: string) => string, tab: PayloadTab) {
  switch (tab) {
    case "originalRequest":
      return t("Original request");
    case "bridgeRequest":
      return t("Bridge request");
    case "serverResponse":
      return t("Server response");
    case "bridgeResponse":
      return t("Bridge response");
    case "serviceSide":
      return t("Multimodal");
    case "search":
      return t("Search");
  }
}

function MetaPill({
  children,
  mono = false,
}: {
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-6 max-w-full items-center gap-1.5 rounded-md border border-border/70 bg-muted/25 px-2 text-[11px] text-muted-foreground",
        mono && "font-mono text-foreground/75",
      )}
    >
      {children}
    </span>
  );
}

function StatusPill({
  status,
  children,
}: {
  status: number;
  children: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-md border px-2 font-mono text-[11px]",
        statusPillClass(status),
      )}
    >
      <span className={cn("h-1.5 w-1.5 rounded-full", statusDotClass(status))} />
      {children}
    </span>
  );
}

function statusPillClass(status: number) {
  if (status >= 200 && status < 300) {
    return "border-emerald-500/25 bg-emerald-500/5 text-emerald-700 dark:text-emerald-400";
  }
  if (status >= 300 && status < 400) {
    return "border-sky-500/25 bg-sky-500/5 text-sky-700 dark:text-sky-400";
  }
  if (status >= 400) {
    return "border-destructive/30 bg-destructive/5 text-destructive";
  }
  return "border-border bg-muted/25 text-muted-foreground";
}

function statusDotClass(status: number) {
  if (status >= 200 && status < 300) return "bg-emerald-500";
  if (status >= 300 && status < 400) return "bg-sky-500";
  if (status >= 400) return "bg-destructive";
  return "bg-muted-foreground/60";
}

function mergeRecordEvent(records: BridgeRecordEntry[], event: BridgeRecordEvent) {
  const next = records.map((record) => {
    if (record.recordId !== event.recordId) return record;
    return mergeIntoRecord(record, event);
  });
  if (!next.some((record) => record.recordId === event.recordId)) {
    next.push(mergeIntoRecord(newRecord(event), event));
  }
  next.sort((a, b) => a.timestampMs - b.timestampMs);
  return next.slice(-MAX_RECORDS);
}

function newRecord(event: BridgeRecordEvent): BridgeRecordEntry {
  return {
    recordId: event.recordId,
    requestId: event.requestId,
    timestampMs: event.timestampMs,
    updatedAtMs: event.timestampMs,
    serviceSidePayloads: [],
    searchPayloads: [],
    errors: [],
    phases: new Set(),
  };
}

function mergeIntoRecord(
  record: BridgeRecordEntry,
  event: BridgeRecordEvent,
): BridgeRecordEntry {
  const phases = new Set(record.phases);
  phases.add(event.phase);
  return {
    ...record,
    requestId: event.requestId,
    updatedAtMs: event.timestampMs,
    metadata: event.metadata ?? record.metadata,
    originalRequest: event.originalRequest ?? record.originalRequest,
    bridgeRequest: event.bridgeRequest ?? record.bridgeRequest,
    serverResponse: event.serverResponse ?? record.serverResponse,
    bridgeResponse: event.bridgeResponse ?? record.bridgeResponse,
    serviceSidePayloads: event.serviceSide
      ? [...record.serviceSidePayloads, event.serviceSide]
      : record.serviceSidePayloads,
    searchPayloads: event.search
      ? [...record.searchPayloads, event.search]
      : record.searchPayloads,
    serverStatus:
      event.phase === "serverResponse" && event.status
        ? event.status
        : record.serverStatus,
    bridgeStatus:
      event.phase === "bridgeResponse" && event.status
        ? event.status
        : record.bridgeStatus,
    errors: event.error ? [...record.errors, event.error] : record.errors,
    phases,
  };
}

function payloadText(payload: RecordedPayload) {
  if (payload.json !== undefined) {
    try {
      return JSON.stringify(payload.json, null, 2);
    } catch {
      return payload.text;
    }
  }
  return payload.text;
}

async function copyPayloadToClipboard(
  payload: RecordedPayload,
  onCopied: () => void,
) {
  try {
    if (!navigator.clipboard) return;
    await navigator.clipboard.writeText(payloadText(payload));
    onCopied();
  } catch {
    // Keep the recorder UI quiet if clipboard permission is unavailable.
  }
}

function formatTime(timestampMs: number) {
  return new Date(timestampMs).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
