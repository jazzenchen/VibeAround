"use client";

import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  ArrowUp,
  Check,
  ChevronDown,
  Command as CommandIcon,
  Hand,
  Paperclip,
  ShieldAlert,
  ShieldCheck,
  Square,
  X,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { CHAT_ATTACHMENT_ACCEPT } from "./attachmentTypes";
import type { ChatAttachment, SessionModeState } from "./chatTypes";

export type { ChatSessionSelection } from "./chatTypes";

const TEXTAREA_MAX_HEIGHT_PX = 256;
const TEXTAREA_MIN_HEIGHT_PX = 72;
const HERO_TEXTAREA_MIN_HEIGHT_PX = 128;

const CHAT_COMMANDS = [
  { label: "/status", value: "/status" },
  { label: "/workspace", value: "/workspace" },
  { label: "/workspace --switch", value: "/workspace --switch " },
  { label: "/agent", value: "/agent" },
  { label: "/agent --switch", value: "/agent --switch " },
  { label: "/agent /help", value: "/agent /help" },
  { label: "/profile", value: "/profile" },
  { label: "/profile --switch", value: "/profile --switch " },
  { label: "/session", value: "/session" },
  { label: "/session --switch", value: "/session --switch " },
];

export interface ChatInputProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  disabled?: boolean;
  submitDisabled?: boolean;
  isStreaming?: boolean;
  onStop?: () => void;
  attachments?: ChatAttachment[];
  attachmentsUploading?: boolean;
  attachmentsUploadingCount?: number;
  attachmentError?: string;
  sendWithModifierEnter?: boolean;
  showCommands?: boolean;
  contextContent?: ReactNode;
  leadingAction?: ReactNode;
  sessionMode?: SessionModeState | null;
  onSessionModeChange?: (value: string) => void;
  onFilesSelected?: (files: File[]) => void;
  onRemoveAttachment?: (id: string) => void;
  placeholder?: string;
  /** Shown at bottom-left as the current chat target. */
  targetLabel?: string;
  variant?: "dock" | "hero";
  className?: string;
}

export function ChatInput({
  value,
  onChange,
  onSubmit,
  disabled = false,
  submitDisabled = false,
  isStreaming = false,
  onStop,
  attachments = [],
  attachmentsUploading = false,
  attachmentsUploadingCount = 0,
  attachmentError,
  sendWithModifierEnter = false,
  showCommands = true,
  contextContent,
  leadingAction,
  sessionMode,
  onSessionModeChange,
  onFilesSelected,
  onRemoveAttachment,
  placeholder = "Message Claude…",
  targetLabel = "Claude Code",
  variant = "dock",
  className,
}: ChatInputProps) {
  const { t } = useI18n();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const isComposingRef = useRef(false);
  const [dragDepth, setDragDepth] = useState(0);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    const minHeight =
      variant === "hero" ? HERO_TEXTAREA_MIN_HEIGHT_PX : TEXTAREA_MIN_HEIGHT_PX;
    el.style.height = "auto";
    el.style.height = `${Math.max(
      minHeight,
      Math.min(el.scrollHeight, TEXTAREA_MAX_HEIGHT_PX),
    )}px`;
  }, [value, variant]);

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const isComposing =
      isComposingRef.current || e.nativeEvent.isComposing || e.keyCode === 229;
    if (isComposing || e.key !== "Enter") return;
    if (sendWithModifierEnter) {
      if (e.metaKey || e.ctrlKey) {
        e.preventDefault();
        if (!disabled && !submitDisabled && canSend) onSubmit();
      }
      return;
    }
    if (!e.shiftKey) {
      e.preventDefault();
      if (!disabled && !submitDisabled && canSend) onSubmit();
    }
  };

  const handleFileChange = (event: ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? []);
    if (files.length > 0) {
      onFilesSelected?.(files);
    }
    event.target.value = "";
  };

  const handleDragEnter = (event: DragEvent<HTMLDivElement>) => {
    if (!eventHasFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    setDragDepth((depth) => depth + 1);
  };

  const handleDragOver = (event: DragEvent<HTMLDivElement>) => {
    if (!eventHasFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect =
      disabled || attachmentsUploading || !onFilesSelected ? "none" : "copy";
  };

  const handleDragLeave = (event: DragEvent<HTMLDivElement>) => {
    if (!eventHasFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    setDragDepth((depth) => Math.max(0, depth - 1));
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    if (!eventHasFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    setDragDepth(0);
    if (disabled || attachmentsUploading || !onFilesSelected) return;
    const files = Array.from(event.dataTransfer.files ?? []);
    if (files.length > 0) {
      onFilesSelected(files);
    }
  };

  const canSend =
    !disabled &&
    !submitDisabled &&
    !attachmentsUploading &&
    (!!value.trim() || attachments.length > 0);
  const showStop = isStreaming && onStop;
  const showDropTarget =
    dragDepth > 0 && Boolean(onFilesSelected) && !disabled && !attachmentsUploading;
  const selectedMode = sessionMode?.options.find(
    (option) => option.value === sessionMode.currentValue,
  );
  const canSelectMode = Boolean(
    sessionMode && selectedMode && onSessionModeChange && !disabled,
  );
  const uploadingLabel =
    attachmentsUploading && attachmentsUploadingCount > 1
      ? t("Uploading {{count}} files…", { count: attachmentsUploadingCount })
      : t("Uploading…");

  return (
    <div
      data-slot="chat-input"
      className={cn(
        variant === "hero" ? "bg-transparent p-0" : "bg-background px-4 pb-4 pt-2",
        className,
      )}
    >
      <div
        role="group"
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        className={cn(
          "relative mx-auto flex max-w-4xl flex-col rounded-lg border border-border transition-[box-shadow,border-color,background-color] focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/30",
          variant === "hero"
            ? "min-h-[11rem] bg-background shadow-lg shadow-foreground/5"
            : "min-h-[5.5rem] bg-muted/30",
          showDropTarget && "border-primary/70 bg-primary/5 ring-2 ring-primary/25",
        )}
      >
        {showDropTarget && (
          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg border border-primary/40 bg-background/85 backdrop-blur-sm">
            <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/10 px-3 py-2 text-xs font-medium text-primary shadow-sm">
              <Paperclip className="h-4 w-4" />
              {t("Drop files to attach")}
            </div>
          </div>
        )}
        {contextContent}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onCompositionStart={() => {
            isComposingRef.current = true;
          }}
          onCompositionEnd={() => {
            isComposingRef.current = false;
          }}
          placeholder={placeholder}
          disabled={disabled}
          rows={1}
          className={cn(
            "max-h-64 resize-none overflow-y-auto border-0 bg-transparent text-base sm:text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-0 transition-[height] duration-200 ease-out",
            variant === "hero"
              ? "min-h-32 px-4 py-3"
              : "min-h-[4.5rem] px-3 py-3",
          )}
          style={{ height: variant === "hero" ? "8rem" : "4.5rem" }}
        />
        {(attachments.length > 0 || attachmentsUploading || attachmentError) && (
          <div className="space-y-1.5 border-t border-border/60 px-3 py-2">
            <div className="flex flex-wrap gap-1.5">
              {attachments.map((attachment) => (
                <span
                  key={attachment.id}
                  className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border/70 bg-background/70 px-2 py-1 text-xs text-muted-foreground"
                  title={attachment.name}
                >
                  <Paperclip className="h-3 w-3 shrink-0" />
                  <span className="min-w-0 truncate text-foreground">
                    {attachment.name}
                  </span>
                  <span className="shrink-0 text-muted-foreground/60">
                    {formatBytes(attachment.size)}
                  </span>
                  {onRemoveAttachment && (
                    <button
                      type="button"
                      className="ml-0.5 rounded-sm text-muted-foreground/70 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      onClick={() => onRemoveAttachment(attachment.id)}
                      aria-label={t("Remove attachment")}
                      title={t("Remove attachment")}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  )}
                </span>
              ))}
              {attachmentsUploading && (
                <span className="rounded-md border border-border/70 bg-background/70 px-2 py-1 text-xs text-muted-foreground">
                  {uploadingLabel}
                </span>
              )}
            </div>
            {attachmentError && (
              <p className="text-xs text-destructive">{attachmentError}</p>
            )}
          </div>
        )}
        <div
          className={cn(
            "flex shrink-0 items-center justify-between gap-1.5",
            variant === "hero" ? "px-3 py-2" : "px-2 py-1",
          )}
        >
          <div className="flex min-w-0 items-center gap-1.5">
            {leadingAction}
            {onFilesSelected && (
              <>
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  accept={CHAT_ATTACHMENT_ACCEPT}
                  className="hidden"
                  onChange={handleFileChange}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => fileInputRef.current?.click()}
                  disabled={disabled || attachmentsUploading}
                  className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
                  aria-label={t("Attach files")}
                  title={t("Attach files")}
                >
                  <Paperclip className="h-4 w-4" />
                </Button>
              </>
            )}
            {sessionMode && selectedMode && (
              <SessionModePicker
                mode={sessionMode}
                disabled={!canSelectMode}
                onChange={onSessionModeChange}
              />
            )}
            {showCommands && (
              <CommandPicker
                disabled={disabled}
                onSelect={(command) => {
                  onChange(command);
                  requestAnimationFrame(() => textareaRef.current?.focus());
                }}
              />
            )}
            <span
              className="min-w-0 truncate px-1 text-xs font-medium text-muted-foreground"
              title={targetLabel}
            >
              {targetLabel}
            </span>
          </div>
          <Button
            type="button"
            size="icon"
            onClick={showStop ? onStop : onSubmit}
            disabled={!showStop && !canSend}
            className="h-9 w-9 shrink-0 rounded-full"
            aria-label={showStop ? t("Stop") : t("Send")}
          >
            {showStop ? (
              <Square className="h-4 w-4" />
            ) : (
              <ArrowUp className="h-[18px] w-[18px]" strokeWidth={2.4} />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}

function CommandPicker({
  disabled,
  onSelect,
}: {
  disabled: boolean;
  onSelect: (command: string) => void;
}) {
  const { t } = useI18n();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          disabled={disabled}
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
          aria-label={t("Commands")}
          title={t("Commands")}
        >
          <CommandIcon className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top" className="w-64">
        {CHAT_COMMANDS.map((command) => (
          <DropdownMenuItem
            key={command.label}
            className="font-mono text-xs"
            onSelect={() => onSelect(command.value)}
          >
            {command.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SessionModePicker({
  mode,
  disabled,
  onChange,
}: {
  mode: SessionModeState;
  disabled: boolean;
  onChange?: (value: string) => void;
}) {
  const { t } = useI18n();
  const selected = mode.options.find((option) => option.value === mode.currentValue);
  if (!selected) return null;
  const pickerLabel = mode.name ?? t("Session mode");
  const Icon = sessionModeIcon(selected.value, selected.name);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          disabled={disabled}
          className="flex h-6 max-w-[13rem] shrink items-center gap-1 rounded-sm px-1 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-60"
          aria-label={`${pickerLabel}: ${selected.name}`}
          title={selected.description ?? selected.name}
        >
          <Icon className="h-3.5 w-3.5 shrink-0" />
          <span className="min-w-0 truncate">{selected.name}</span>
          <ChevronDown className="h-3 w-3 shrink-0" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top" className="w-72">
        {mode.options.map((option) => {
          const OptionIcon = sessionModeIcon(option.value, option.name);
          return (
            <DropdownMenuItem
              key={option.value}
              className="flex items-start gap-2 px-2 py-1.5 text-xs"
              onSelect={() => onChange?.(option.value)}
            >
              <OptionIcon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-medium">{option.name}</span>
                {option.description && (
                  <span className="mt-0.5 block line-clamp-3 text-[11px] leading-snug text-muted-foreground">
                    {option.description}
                  </span>
                )}
              </span>
              {option.value === mode.currentValue && (
                <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              )}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function sessionModeIcon(value: string, name: string) {
  const token = `${value} ${name}`.toLowerCase();
  if (
    token.includes("read") ||
    token.includes("ask") ||
    token.includes("view")
  ) {
    return Hand;
  }
  if (
    token.includes("full") ||
    token.includes("bypass") ||
    token.includes("unrestricted") ||
    token.includes("danger")
  ) {
    return ShieldAlert;
  }
  return ShieldCheck;
}

function formatBytes(size: number) {
  if (!Number.isFinite(size) || size <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = size;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function eventHasFiles(event: DragEvent<HTMLElement>) {
  return Array.from(event.dataTransfer.types).includes("Files");
}
