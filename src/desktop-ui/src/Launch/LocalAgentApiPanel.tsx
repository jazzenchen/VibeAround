import {
  type ChangeEvent,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  AlertCircle,
  Check,
  FileUp,
  Loader2,
  MessageSquare,
  Paperclip,
  RefreshCw,
  Send,
  X,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  API_BASE,
  apiFetch,
  getLocalAgentApiToken,
  rotateLocalAgentApiToken,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  LOCAL_API_PROTOCOLS,
  extractLocalAgentModels,
  extractLocalAgentResponseText,
  localAgentBasePath,
  localAgentErrorText,
  localAgentProtocolSpec,
  localAgentTestPayload,
  parseLocalAgentJson,
  type LocalAgentModel,
  type LocalAgentApiTarget,
  type LocalAgentTestAttachment,
  type LocalApiProtocol,
} from "./localAgentApi";
import { ModelIdCombobox } from "./ModelIdCombobox";

const DEFAULT_TEST_PROMPT = "Reply with exactly: VA_LOCAL_API_OK";
const MAX_LOCAL_API_ATTACHMENT_BYTES = 25 * 1024 * 1024;
const MAX_LOCAL_API_ATTACHMENT_TOTAL_BYTES = 40 * 1024 * 1024;

interface LocalAgentApiPanelProps {
  target: LocalAgentApiTarget;
  className?: string;
  serviceEnabled?: boolean;
}

interface TestResult {
  ok: boolean;
  status: number;
  text: string;
}

export function LocalAgentApiPanel({
  target,
  className,
  serviceEnabled = true,
}: LocalAgentApiPanelProps) {
  const { t } = useI18n();
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const isComposingRef = useRef(false);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [protocol, setProtocol] =
    useState<LocalApiProtocol>("openai-responses");
  const [prompt, setPrompt] = useState(DEFAULT_TEST_PROMPT);
  const [model, setModel] = useState("");
  const [modelLoading, setModelLoading] = useState(false);
  const [modelLoadError, setModelLoadError] = useState(false);
  const [modelOptions, setModelOptions] = useState<LocalAgentModel[]>([]);
  const [attachments, setAttachments] = useState<LocalAgentTestAttachment[]>(
    [],
  );
  const [attachmentLoading, setAttachmentLoading] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [dragDepth, setDragDepth] = useState(0);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [clientKey, setClientKey] = useState("");
  const [rotatingKey, setRotatingKey] = useState(false);

  const basePath = localAgentBasePath(target);
  const baseUrl = `${API_BASE}${basePath}`;
  const selectedProtocol = localAgentProtocolSpec(protocol);
  const endpointUrl = selectedProtocol
    ? `${baseUrl}/${selectedProtocol.endpoint}`
    : baseUrl;
  const modelsUrl = `${baseUrl}/models`;
  const showDropTarget = dragDepth > 0;
  const canSendTest =
    serviceEnabled &&
    !testing &&
    !modelLoading &&
    !attachmentLoading &&
    Boolean(model.trim()) &&
    (Boolean(prompt.trim()) || attachments.length > 0);

  useEffect(() => {
    if (!target) return;
    let cancelled = false;
    setProtocol("openai-responses");
    setPrompt(DEFAULT_TEST_PROMPT);
    setModel("");
    setModelLoading(true);
    setModelLoadError(false);
    setModelOptions([]);
    setAttachments([]);
    setAttachmentLoading(false);
    setAttachmentError(null);
    setDragDepth(0);
    setTestResult(null);
    setClientKey("");
    setRotatingKey(false);
    void getLocalAgentApiToken().then((token) => {
      if (!cancelled && token) setClientKey(token);
    });
    if (!serviceEnabled) {
      setModelLoading(false);
      setModelLoadError(true);
      return () => {
        cancelled = true;
      };
    }
    void apiFetch(`${localAgentBasePath(target)}/models`, {
      headers: {
        "x-vibearound-cwd": target.workspacePath,
      },
    })
      .then(async (response) => {
        if (cancelled) return;
        if (!response.ok) {
          setModel(target.agentId);
          setModelOptions([]);
          setModelLoadError(true);
          return;
        }
        const payload = await response.json().catch(() => null);
        if (cancelled) return;
        const models = extractLocalAgentModels(payload);
        setModelOptions(models);
        setModelLoadError(models.length === 0);
        setModel(models[0]?.id ?? target.agentId);
      })
      .catch((error) => {
        if (cancelled) return;
        setModel(target.agentId);
        setModelOptions([]);
        setModelLoadError(true);
        console.warn("[desktop-ui] local agent models fetch failed:", error);
      })
      .finally(() => {
        if (!cancelled) {
          setModelLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [target, serviceEnabled]);

  async function copyValue(key: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(key);
      window.setTimeout(() => {
        setCopiedKey((current) => (current === key ? null : current));
      }, 1400);
    } catch {
      // Clipboard errors are non-fatal; users can still select visible text.
    }
  }

  async function regenerateClientKey() {
    if (rotatingKey) return;
    if (
      !window.confirm(
        t(
          "Generate a new API key? Profiles using the current key stop working until you paste the new one.",
        ),
      )
    ) {
      return;
    }
    setRotatingKey(true);
    try {
      setClientKey(await rotateLocalAgentApiToken());
    } catch (error) {
      console.warn("[desktop-ui] rotate_local_agent_api_token failed:", error);
    } finally {
      setRotatingKey(false);
    }
  }

  async function appendFiles(filesLike: FileList | File[]) {
    if (!serviceEnabled) return;
    const files = Array.from(filesLike);
    if (files.length === 0) return;

    const tooLarge: File[] = [];
    const totalOverflow: File[] = [];
    const readable: File[] = [];
    let nextTotal = attachments.reduce(
      (sum, attachment) => sum + attachment.size,
      0,
    );
    for (const file of files) {
      if (file.size > MAX_LOCAL_API_ATTACHMENT_BYTES) {
        tooLarge.push(file);
        continue;
      }
      if (nextTotal + file.size > MAX_LOCAL_API_ATTACHMENT_TOTAL_BYTES) {
        totalOverflow.push(file);
        continue;
      }
      readable.push(file);
      nextTotal += file.size;
    }

    setAttachmentLoading(readable.length > 0);
    setAttachmentError(null);
    setTestResult(null);

    try {
      const settled = await Promise.allSettled(
        readable.map(readLocalApiAttachment),
      );
      const nextAttachments = settled
        .filter(
          (
            result,
          ): result is PromiseFulfilledResult<LocalAgentTestAttachment> =>
            result.status === "fulfilled",
        )
        .map((result) => result.value);
      if (nextAttachments.length > 0) {
        setAttachments((current) => [...current, ...nextAttachments]);
      }
      const failedCount = settled.length - nextAttachments.length;
      const messages: string[] = [];
      if (tooLarge.length > 0) {
        messages.push(
          t("{{count}} files exceed {{limit}} MB.", {
            count: tooLarge.length,
            limit: Math.round(MAX_LOCAL_API_ATTACHMENT_BYTES / 1024 / 1024),
          }),
        );
      }
      if (totalOverflow.length > 0) {
        messages.push(
          t("Total attachments exceed {{limit}} MB.", {
            limit: Math.round(
              MAX_LOCAL_API_ATTACHMENT_TOTAL_BYTES / 1024 / 1024,
            ),
          }),
        );
      }
      if (failedCount > 0) {
        messages.push(
          t("{{count}} files failed to read.", { count: failedCount }),
        );
      }
      setAttachmentError(messages.join(" "));
    } finally {
      setAttachmentLoading(false);
    }
  }

  function handleFileChange(event: ChangeEvent<HTMLInputElement>) {
    if (event.currentTarget.files) {
      void appendFiles(event.currentTarget.files);
    }
    event.currentTarget.value = "";
  }

  function handleDragEnter(event: DragEvent<HTMLDivElement>) {
    if (!dragEventHasFiles(event)) return;
    event.preventDefault();
    setDragDepth((current) => current + 1);
  }

  function handleDragOver(event: DragEvent<HTMLDivElement>) {
    if (!dragEventHasFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  }

  function handleDragLeave(event: DragEvent<HTMLDivElement>) {
    if (!dragEventHasFiles(event)) return;
    event.preventDefault();
    setDragDepth((current) => Math.max(0, current - 1));
  }

  function handleDrop(event: DragEvent<HTMLDivElement>) {
    if (!dragEventHasFiles(event)) return;
    event.preventDefault();
    setDragDepth(0);
    void appendFiles(event.dataTransfer.files);
  }

  function removeAttachment(id: string) {
    setAttachments((current) => current.filter((item) => item.id !== id));
    setAttachmentError(null);
    setTestResult(null);
  }

  function handlePromptKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.metaKey &&
      !event.ctrlKey &&
      !isComposingRef.current
    ) {
      event.preventDefault();
      if (canSendTest) {
        void runTest();
      }
    }
  }

  async function runTest() {
    if (!target || !selectedProtocol || !serviceEnabled) return;
    setTesting(true);
    setTestResult(null);
    try {
      const response = await apiFetch(
        `${basePath}/${selectedProtocol.endpoint}`,
        {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-vibearound-cwd": target.workspacePath,
          },
          body: JSON.stringify(
            localAgentTestPayload(protocol, model, prompt, attachments),
          ),
        },
      );
      const rawText = await response.text();
      const payload = parseLocalAgentJson(rawText);
      setTestResult({
        ok: response.ok,
        status: response.status,
        text: response.ok
          ? extractLocalAgentResponseText(protocol, payload) || rawText
          : localAgentErrorText(payload, rawText),
      });
    } catch (error) {
      setTestResult({
        ok: false,
        status: 0,
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTesting(false);
    }
  }

  return (
    <section className={cn("grid gap-3", className)}>
      <div className="flex flex-wrap items-center gap-3">
        <Tabs
          value={protocol}
          onValueChange={(value) => {
            setProtocol(value as LocalApiProtocol);
            setTestResult(null);
          }}
        >
          <TabsList className="h-8">
            {LOCAL_API_PROTOCOLS.map((item) => (
              <TabsTrigger
                key={item.id}
                value={item.id}
                className="px-3 text-xs"
              >
                {item.shortLabel}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>

      <div className="grid gap-1 rounded-md border border-border/70 p-2">
        <div className="flex min-h-5 flex-wrap items-center gap-2">
          <div className="text-xs font-semibold">
            {t("Manual configuration")}
          </div>
          <div className="text-[11px] text-muted-foreground">
            {t("Click a value to copy.")}
          </div>
        </div>
        <ManualField
          label={t("Base URL")}
          value={endpointUrl}
          copied={copiedKey === protocol}
          onCopy={() => copyValue(protocol, endpointUrl)}
        />
        <ManualField
          label={t("Models API")}
          value={modelsUrl}
          copied={copiedKey === "models"}
          onCopy={() => copyValue("models", modelsUrl)}
        />
        {clientKey && (
          <>
            <ManualField
              label={t("API key")}
              value={clientKey}
              copied={copiedKey === "api-key"}
              onCopy={() => copyValue("api-key", clientKey)}
              action={
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  className="h-5 w-5 shrink-0 text-muted-foreground hover:text-foreground"
                  aria-label={t("Generate a new API key")}
                  title={t("Generate a new API key")}
                  disabled={rotatingKey}
                  onClick={regenerateClientKey}
                >
                  {rotatingKey ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <RefreshCw className="h-3 w-3" />
                  )}
                </Button>
              }
            />
            <div className="pl-[86px] text-[11px] leading-4 text-muted-foreground">
              {t("This key stays the same across restarts.")}
            </div>
          </>
        )}
        <ModelListField
          label={t("Models")}
          models={modelOptions}
          fallback={
            !serviceEnabled
              ? t("Local API service disabled")
              : modelLoading
                ? t("Loading…")
                : modelLoadError
                  ? t("Failed to fetch model list")
                  : model || target.agentId
          }
          copiedKey={copiedKey}
          onCopy={(modelId) => copyValue(`model:${modelId}`, modelId)}
        />
      </div>

      <div className="grid gap-2 rounded-md border border-border/70 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <MessageSquare className="h-4 w-4 text-primary" />
            {t("Test message")}
          </div>
        </div>
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] gap-2">
          <div className="grid gap-1 text-[11px] text-muted-foreground">
            <span>{t("Model")}</span>
            <ModelIdCombobox
              value={model}
              options={modelOptions.map((option) => ({
                id: option.id,
              }))}
              onValueChange={setModel}
              placeholder={modelLoading ? t("Loading…") : undefined}
              inputClassName="h-7 w-full font-mono text-xs"
              dropdownClassName="max-h-36"
              disabled={modelLoading || !serviceEnabled}
            />
          </div>
          <label className="grid gap-1 text-[11px] text-muted-foreground">
            <span>{t("Workspace")}</span>
            <Input
              value={target.workspacePath}
              readOnly
              className="h-7 font-mono text-xs"
            />
          </label>
        </div>
        <div
          role="group"
          className={cn(
            "relative flex min-h-[96px] flex-col rounded-md border border-input bg-background/70 p-2 shadow-xs transition-[border-color,box-shadow,background-color] focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/25",
            showDropTarget &&
              "border-primary/70 bg-primary/5 ring-2 ring-primary/25",
          )}
          onDragEnter={handleDragEnter}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
        >
          {showDropTarget && (
            <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-md border border-primary/40 bg-background/85 backdrop-blur-sm">
              <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/10 px-3 py-2 text-xs font-medium text-primary shadow-sm">
                <Paperclip className="h-4 w-4" />
                {t("Drop files to attach")}
              </div>
            </div>
          )}
          {(attachments.length > 0 || attachmentLoading || attachmentError) && (
            <div className="space-y-1.5 px-1 pb-1">
              <div className="flex flex-wrap gap-1.5">
                {attachments.map((attachment) => (
                  <span
                    key={attachment.id}
                    className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border/70 bg-muted/30 px-2 py-0.5 text-[11px] text-muted-foreground"
                    title={attachment.name}
                  >
                    <Paperclip className="h-3.5 w-3.5 shrink-0" />
                    <span className="min-w-0 truncate text-foreground">
                      {attachment.name}
                    </span>
                    <span className="shrink-0 text-muted-foreground/70">
                      {formatAttachmentSize(attachment.size)}
                    </span>
                    <button
                      type="button"
                      className="ml-0.5 rounded-sm text-muted-foreground/70 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      onClick={() => removeAttachment(attachment.id)}
                      aria-label={t("Remove attachment")}
                      title={t("Remove attachment")}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </span>
                ))}
                {attachmentLoading && (
                  <span className="rounded-md border border-border/70 bg-muted/30 px-2 py-0.5 text-[11px] text-muted-foreground">
                    {t("Loading…")}
                  </span>
                )}
              </div>
              {attachmentError && (
                <p className="text-[11px] text-destructive">
                  {attachmentError}
                </p>
              )}
            </div>
          )}
          <textarea
            value={prompt}
            aria-label={t("Message")}
            placeholder={t("Message")}
            disabled={!serviceEnabled}
            onChange={(event) => {
              setPrompt(event.currentTarget.value);
              setTestResult(null);
            }}
            onKeyDown={handlePromptKeyDown}
            onCompositionStart={() => {
              isComposingRef.current = true;
            }}
            onCompositionEnd={() => {
              isComposingRef.current = false;
            }}
            className="min-h-[48px] w-full flex-1 resize-none border-0 bg-transparent px-1 py-1 text-xs text-foreground outline-none placeholder:text-muted-foreground focus:ring-0 disabled:cursor-not-allowed disabled:opacity-60"
          />
          <div className="flex shrink-0 items-center justify-between gap-2 pt-1">
            <div className="flex min-w-0 items-center gap-1.5">
              <input
                ref={fileInputRef}
                type="file"
                multiple
                className="hidden"
                onChange={handleFileChange}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                disabled={!serviceEnabled || testing || attachmentLoading}
                aria-label={t("Attach files")}
                title={t("Attach files")}
                onClick={() => fileInputRef.current?.click()}
              >
                <FileUp className="h-4 w-4" />
              </Button>
            </div>
            <Button
              type="button"
              size="sm"
              disabled={!canSendTest}
              onClick={() => void runTest()}
            >
              {testing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Send className="h-3.5 w-3.5" />
              )}
              {testing ? t("Testing…") : t("Send test")}
            </Button>
          </div>
        </div>
        {testResult && (
          <div
            className={`grid gap-1 rounded-md border p-2 ${
              testResult.ok
                ? "border-emerald-500/25 bg-emerald-500/5"
                : "border-destructive/25 bg-destructive/5"
            }`}
          >
            <div
              className={`flex items-center gap-1.5 text-[11px] font-medium ${
                testResult.ok ? "text-emerald-600" : "text-destructive"
              }`}
            >
              {testResult.ok ? (
                <Check className="h-3.5 w-3.5" />
              ) : (
                <AlertCircle className="h-3.5 w-3.5" />
              )}
              {t("HTTP {{status}}", { status: testResult.status })}
            </div>
            <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-words rounded bg-background/70 p-2 font-mono text-[11px] leading-5 text-foreground">
              {testResult.text || t("Empty")}
            </pre>
          </div>
        )}
      </div>
    </section>
  );
}

function ManualField({
  label,
  value,
  copied,
  onCopy,
  tone = "default",
  action,
}: {
  label: string;
  value: string;
  copied: boolean;
  onCopy: () => void;
  tone?: "default" | "primary";
  action?: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="grid grid-cols-[78px_minmax(0,1fr)] items-center gap-2">
      <div className="min-w-0 text-[11px] leading-5 text-muted-foreground">
        <span className="truncate">{label}</span>
      </div>
      <div className="flex min-w-0 items-center gap-1">
        <button
          type="button"
          className={`flex h-5 w-full min-w-0 cursor-pointer items-center rounded px-1.5 text-left font-mono text-[11px] leading-5 transition-colors ${
            tone === "primary"
              ? "bg-primary/5 text-primary hover:bg-primary/10"
              : "bg-muted/35 text-foreground hover:bg-muted/60"
          }`}
          aria-label={t("Copy")}
          title={t("Copy")}
          onClick={onCopy}
        >
          <span className="min-w-0 truncate">{value}</span>
          {copied && (
            <span className="ml-2 shrink-0 text-[11px] font-sans text-primary">
              {t("Copied")}
            </span>
          )}
        </button>
        {action}
      </div>
    </div>
  );
}

function ModelListField({
  label,
  models,
  fallback,
  copiedKey,
  onCopy,
}: {
  label: string;
  models: LocalAgentModel[];
  fallback: string;
  copiedKey: string | null;
  onCopy: (modelId: string) => void;
}) {
  const { t } = useI18n();
  if (models.length === 0) {
    return (
      <div className="grid grid-cols-[78px_minmax(0,1fr)] items-start gap-2">
        <div className="min-w-0 text-[11px] leading-5 text-muted-foreground">
          <span className="truncate">{label}</span>
        </div>
        <div className="flex h-5 min-w-0 items-center rounded bg-muted/35 px-1.5 font-mono text-[11px] leading-5 text-foreground">
          <span className="min-w-0 truncate">{fallback}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-[78px_minmax(0,1fr)] items-start gap-2">
      <div className="min-w-0 text-[11px] leading-5 text-muted-foreground">
        <span className="truncate">{label}</span>
      </div>
      <div className="flex min-h-5 min-w-0 flex-wrap items-center gap-1 rounded bg-muted/35 px-1.5 py-0.5">
        {models.map((model) => (
          <button
            key={model.id}
            type="button"
            className="inline-flex h-4 max-w-[220px] shrink-0 cursor-pointer items-center rounded px-1 text-left text-[11px] leading-4 transition-colors hover:bg-muted/60"
            aria-label={t("Copy")}
            title={model.id}
            onClick={() => onCopy(model.id)}
          >
            <span className="min-w-0 truncate font-mono text-foreground">
              {model.id}
            </span>
            {copiedKey === `model:${model.id}` && (
              <span className="ml-1.5 shrink-0 text-primary">
                {t("Copied")}
              </span>
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

function dragEventHasFiles(event: DragEvent<HTMLElement>): boolean {
  return Array.from(event.dataTransfer.types).includes("Files");
}

function readLocalApiAttachment(file: File): Promise<LocalAgentTestAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () =>
      reject(reader.error ?? new Error("File read failed"));
    reader.onload = () => {
      if (typeof reader.result !== "string") {
        reject(new Error("File read failed"));
        return;
      }
      resolve({
        id: createAttachmentId(file),
        name: file.name || "attachment",
        mimeType: file.type || "application/octet-stream",
        size: file.size,
        dataUrl: reader.result,
      });
    };
    reader.readAsDataURL(file);
  });
}

function createAttachmentId(file: File): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${file.name}-${file.size}`;
}

function formatAttachmentSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.ceil(size / 1024)} KB`;
  return `${(size / 1024 / 1024).toFixed(size < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}
