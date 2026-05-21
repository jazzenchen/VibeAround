import {
  type ComponentProps,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from "react";
import { useSortable } from "@dnd-kit/react/sortable";
import {
  Copy,
  GripVertical,
  MoreVertical,
  Pencil,
  Plug,
  Star,
  Trash2,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { AgentSummary, WorkspaceOption } from "./api";
import type { ProfileSummary } from "./types";

export function SelectorTile({
  active,
  onClick,
  icon,
  label,
  title,
  detail,
  badges,
  disabled = false,
  disabledReason,
}: {
  active: boolean;
  onClick?: () => void;
  icon: ReactNode;
  label: string;
  title: string;
  detail: string;
  badges?: ReactNode;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const className = `flex min-h-[62px] items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors ${
    disabled
      ? "cursor-not-allowed border-border bg-card text-muted-foreground opacity-60"
      : active
        ? "cursor-pointer border-primary bg-primary/10 shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.35)]"
        : onClick
          ? "cursor-pointer border-border bg-card hover:border-primary/40"
          : "border-border bg-card"
  }`;
  const content = (
    <>
      <span
        className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md border ${
          active
            ? "border-primary/40 bg-background text-primary"
            : "border-border/70 bg-background text-muted-foreground"
        }`}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span
            className={`text-[10px] font-semibold uppercase ${active ? "text-primary" : "text-muted-foreground"}`}
          >
            {label}
          </span>
          {badges}
        </span>
        <span className="block truncate text-[12px] font-semibold">{title}</span>
        <span className="block truncate text-[10px] text-muted-foreground">
          {detail}
        </span>
      </span>
    </>
  );

  if (!onClick) {
    return <div className={className}>{content}</div>;
  }

  const tile = (
    <button
      type="button"
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      title={disabled ? disabledReason : undefined}
      onClick={() => {
        if (!disabled) onClick();
      }}
      className={className}
    >
      {content}
    </button>
  );
  if (!disabled || !disabledReason) return tile;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{tile}</TooltipTrigger>
      <TooltipContent>{disabledReason}</TooltipContent>
    </Tooltip>
  );
}

export function SelectableItemCard({
  active,
  disabled = false,
  isDragging = false,
  children,
  onSelect,
}: {
  active: boolean;
  disabled?: boolean;
  isDragging?: boolean;
  children: ReactNode;
  onSelect: () => void;
}) {
  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (disabled || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    onSelect();
  }

  return (
    <div
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-disabled={disabled}
      onClick={() => {
        if (!disabled) onSelect();
      }}
      onKeyDown={handleKeyDown}
      className={`flex h-full min-h-[94px] w-full items-center gap-3 rounded-md border px-3.5 py-3 text-left transition-colors ${
        active
          ? "border-primary bg-primary/10 text-primary shadow-[inset_3px_0_0_hsl(var(--primary))]"
          : disabled
            ? "border-border bg-card"
            : "border-border bg-card hover:border-primary/40 hover:bg-accent/35"
      } ${disabled ? "cursor-not-allowed" : "cursor-pointer"} ${
        isDragging ? "opacity-55" : ""
      }`}
    >
      {children}
    </div>
  );
}

export function SortableItem({
  id,
  index,
  disabled,
  children,
}: {
  id: string;
  index: number;
  disabled: boolean;
  children: (props: {
    dragHandleRef: Ref<HTMLSpanElement>;
    isDragging: boolean;
  }) => ReactNode;
}) {
  const { ref, handleRef, isDragging, isDropTarget } = useSortable({
    id,
    index,
    disabled,
  });

  return (
    <div
      ref={ref}
      className={`relative rounded-md transition-shadow ${
        isDropTarget ? "ring-2 ring-primary/35 shadow-lg shadow-primary/20" : ""
      }`}
    >
      {children({
        dragHandleRef: handleRef as Ref<HTMLSpanElement>,
        isDragging,
      })}
    </div>
  );
}

export function DragHandle({
  label,
  disabled = false,
  disabledReason,
  dragHandleRef,
}: {
  label: string;
  disabled?: boolean;
  disabledReason?: string;
  dragHandleRef?: Ref<HTMLSpanElement>;
}) {
  const { t } = useI18n();
  const tooltip = disabled
    ? (disabledReason ?? t("This item cannot be reordered"))
    : label;
  const handle = (
    <span
      ref={disabled ? undefined : dragHandleRef}
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-disabled={disabled}
      title={tooltip}
      onClick={(event) => event.stopPropagation()}
      className={`flex h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground ${
        disabled
          ? "cursor-not-allowed opacity-35"
          : "cursor-grab hover:bg-muted hover:text-foreground active:cursor-grabbing"
      }`}
    >
      <GripVertical className="h-4 w-4" />
    </span>
  );

  if (!disabled) return handle;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{handle}</TooltipTrigger>
      <TooltipContent>{tooltip}</TooltipContent>
    </Tooltip>
  );
}

export function DisabledMoreButton({ reason }: { reason?: string }) {
  const { t } = useI18n();
  const tooltip = reason ?? t("No actions available");
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="inline-flex cursor-not-allowed"
          tabIndex={-1}
          role="button"
          aria-disabled="true"
          aria-label={tooltip}
          title={tooltip}
        >
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            className="h-7 w-7 text-muted-foreground"
            disabled
            aria-label={tooltip}
            title={tooltip}
          >
            <MoreVertical className="h-3.5 w-3.5" />
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent>{tooltip}</TooltipContent>
    </Tooltip>
  );
}

export function TooltipButton({
  disabledReason,
  disabled,
  children,
  ...props
}: ComponentProps<typeof Button> & { disabledReason?: string }) {
  const button = (
    <Button {...props} disabled={disabled}>
      {children}
    </Button>
  );
  if (!disabled || !disabledReason) return button;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="inline-flex"
          tabIndex={-1}
          aria-disabled="true"
          title={disabledReason}
        >
          {button}
        </span>
      </TooltipTrigger>
      <TooltipContent>{disabledReason}</TooltipContent>
    </Tooltip>
  );
}

export function ProfileActionsMenu({
  profile,
  bridgeAvailable,
  onMakeDefault,
  makeDefaultDisabled = false,
  onConnectionSettings,
  onEditProfile,
  onCopyProfile,
  onDeleteProfile,
}: {
  profile: ProfileSummary;
  bridgeAvailable: boolean;
  onMakeDefault?: () => void;
  makeDefaultDisabled?: boolean;
  onConnectionSettings: (profile: ProfileSummary) => void;
  onEditProfile: (profile: ProfileSummary) => void;
  onCopyProfile: (profile: ProfileSummary) => void;
  onDeleteProfile: (profile: ProfileSummary) => void;
}) {
  const { t } = useI18n();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="h-7 w-7 text-muted-foreground"
          aria-label={t("More")}
        >
          <MoreVertical className="h-3.5 w-3.5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-40">
        {onMakeDefault && (
          <DropdownMenuItem
            className="text-xs"
            disabled={makeDefaultDisabled}
            onSelect={() => onMakeDefault()}
          >
            <Star className="h-3 w-3" />
            {t("Set app default")}
          </DropdownMenuItem>
        )}
        {onMakeDefault && <DropdownMenuSeparator />}
        {bridgeAvailable && (
          <DropdownMenuItem
            className="text-xs"
            onSelect={() => onConnectionSettings(profile)}
          >
            <Plug className="h-3 w-3" />
            {t("API bridge")}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem
          className="text-xs"
          onSelect={() => onEditProfile(profile)}
        >
          <Pencil className="h-3 w-3" />
          {t("Edit")}
        </DropdownMenuItem>
        <DropdownMenuItem
          className="text-xs"
          onSelect={() => onCopyProfile(profile)}
        >
          <Copy className="h-3 w-3" />
          {t("Copy")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className="text-xs"
          variant="destructive"
          onSelect={() => onDeleteProfile(profile)}
        >
          <Trash2 className="h-3 w-3" />
          {t("Delete")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function DirectProfileActionsMenu({
  isDefault,
  disabled,
  onMakeDefault,
}: {
  isDefault: boolean;
  disabled: boolean;
  onMakeDefault: () => void;
}) {
  const { t } = useI18n();
  if (isDefault) {
    return (
      <DisabledMoreButton reason={t("Direct profile cannot be edited or deleted")} />
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="h-7 w-7 text-muted-foreground"
          aria-label={t("More")}
        >
          <MoreVertical className="h-3.5 w-3.5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-40">
        <DropdownMenuItem
          className="text-xs"
          disabled={disabled}
          onSelect={() => onMakeDefault()}
        >
          <Star className="h-3 w-3" />
          {t("Set app default")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="text-xs" disabled>
          <Pencil className="h-3 w-3" />
          {t("Direct profile is fixed")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function WorkspaceActionsMenu({
  workspace,
  onDelete,
}: {
  workspace: WorkspaceOption;
  onDelete: (workspace: WorkspaceOption) => void;
}) {
  const { t } = useI18n();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="h-7 w-7 text-muted-foreground"
          aria-label={t("More")}
        >
          <MoreVertical className="h-3.5 w-3.5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        <DropdownMenuItem
          className="text-xs"
          variant="destructive"
          onSelect={() => onDelete(workspace)}
        >
          <Trash2 className="h-3 w-3" />
          {t("Delete")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function DefaultBadge() {
  const { t } = useI18n();
  return (
    <span className="inline-flex items-center gap-1 rounded border border-amber-500/35 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
      <Star className="h-3 w-3" />
      {t("Default")}
    </span>
  );
}

export function BridgeBadge() {
  const { t } = useI18n();
  return (
    <span className="inline-flex items-center gap-1 rounded border border-primary/25 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
      <Plug className="h-3 w-3" />
      {t("API bridge on")}
    </span>
  );
}

export function AgentRailButton({
  agent,
  active,
  isDefault,
  onClick,
}: {
  agent: AgentSummary;
  active: boolean;
  isDefault: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={agent.display_name}
      className={`relative flex h-14 w-14 cursor-pointer items-center justify-center rounded-md border transition-colors ${
        active
          ? "border-primary bg-primary/10 text-primary"
          : "border-border bg-background text-muted-foreground hover:border-primary/40 hover:text-foreground"
      }`}
    >
      <BrandIcon
        kind="cli"
        id={agent.id}
        label={agent.display_name}
        framed={false}
        className="h-9 w-9"
      />
      {isDefault && (
        <span className="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full border border-amber-500/40 bg-background text-amber-600 shadow-sm dark:text-amber-300">
          <Star className="h-2.5 w-2.5" />
        </span>
      )}
    </button>
  );
}
