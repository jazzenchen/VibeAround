import { type KeyboardEvent, type ReactNode, type Ref } from "react";
import { DragDropProvider, type DragEndEvent } from "@dnd-kit/react";
import { isSortable } from "@dnd-kit/react/sortable";
import {
  Archive,
  Check,
  FolderOpen,
  History,
  MessageCircle,
  Plus,
  Terminal,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { resolveProfileConnection } from "./connections";
import type {
  LaunchSessionSummary,
  LauncherPreferences,
  TerminalOption,
  WorkspaceOption,
} from "./api";
import {
  agentConnectionDef,
  apiTypeProtocolDisplayLabel,
  canDeleteWorkspace,
  isGlobalDefaultDirect,
  isGlobalDefaultProfile,
  isBridgeAgent,
  isSortableWorkspace,
  profileAvailability,
  profileSummary,
  relativeTime,
  type ProfileChoice,
  type SessionChoice,
} from "./launchModel";
import {
  DefaultBadge,
  DisabledMoreButton,
  DirectProfileActionsMenu,
  DragHandle,
  ProfileActionsMenu,
  BridgeBadge,
  SelectableItemCard,
  SortableItem,
  WorkspaceActionsMenu,
} from "./LaunchBuilderPrimitives";
import type { ConnectionAgentId, ProfileSummary } from "./types";

function SelectorPanelShell({
  title,
  headerExtra,
  footer,
  children,
}: {
  title: string;
  headerExtra?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="box-border flex max-h-[430px] flex-col overflow-hidden rounded-md border border-border bg-card shadow-sm">
      <div className="flex items-center justify-between gap-3 border-b border-border/70 px-3 py-2">
        <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/70">
          {title}
        </div>
        {headerExtra}
      </div>
      {children}
      {footer && (
        <div className="shrink-0 border-t border-border bg-background">
          {footer}
        </div>
      )}
    </section>
  );
}

export function TerminalPanel({
  options,
  selected,
  busy,
  onSelect,
}: {
  options: TerminalOption[];
  selected: string;
  busy: boolean;
  onSelect: (terminalId: string) => void;
}) {
  const { t } = useI18n();
  return (
    <SelectorPanelShell title={t("Switch terminal")}>
      <div className="min-h-0 flex-1 divide-y divide-border/60 overflow-y-auto">
        {options.map((option) => {
          const active = option.id === selected;
          const disabled = busy || !option.installed;
          return (
            <button
              key={option.id}
              type="button"
              disabled={disabled}
              className={`flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${
                active
                  ? "bg-primary/10 text-primary"
                  : "text-foreground hover:bg-accent/50"
              }`}
              onClick={() => onSelect(option.id)}
            >
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
                <Terminal className="h-4 w-4" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-semibold">
                  {option.label}
                </span>
                {!option.installed && (
                  <span className="block truncate text-[11px] text-muted-foreground">
                    {t("{{label}} (not installed)", { label: option.label })}
                  </span>
                )}
              </span>
              {active ? (
                <Check className="h-4 w-4 shrink-0 text-primary" />
              ) : (
                <span className="h-4 w-4 shrink-0" aria-hidden="true" />
              )}
            </button>
          );
        })}
      </div>
    </SelectorPanelShell>
  );
}

export function ProfilePanel({
  agentId,
  prefs,
  selected,
  profiles,
  onSelect,
  onSelectApiType,
  onMakeDefault,
  onEditProfile,
  onCopyProfile,
  onConnectionSettings,
  onDeleteProfile,
  onReorderProfile,
  onNewProfile,
  busy,
}: {
  agentId: string;
  prefs: LauncherPreferences;
  selected: ProfileChoice;
  profiles: ProfileSummary[];
  onSelect: (choice: ProfileChoice) => void;
  onSelectApiType: (profile: ProfileSummary, apiType: string) => void;
  onMakeDefault: (choice: ProfileChoice) => Promise<void>;
  onEditProfile: (profile: ProfileSummary) => void;
  onCopyProfile: (profile: ProfileSummary) => void;
  onConnectionSettings: (
    profile: ProfileSummary,
    agentId: ConnectionAgentId,
  ) => void;
  onDeleteProfile: (profile: ProfileSummary) => void;
  onReorderProfile: (fromId: string, toId: string) => void;
  onNewProfile: () => void;
  busy: boolean;
}) {
  const { t } = useI18n();
  const directIsGlobalDefault = isGlobalDefaultDirect(prefs, agentId);
  const directActive = selected.kind === "direct";

  function handleProfileDragEnd(event: DragEndEvent) {
    if (event.canceled || busy) return;
    const { source } = event.operation;
    if (!isSortable(source) || source.initialIndex === source.index) return;
    const from = profiles[source.initialIndex]?.id;
    const to = profiles[source.index]?.id;
    if (from && to) onReorderProfile(from, to);
  }

  return (
    <section className="space-y-2">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-2">
        <SelectableItemCard
          active={false}
          disabled={busy}
          onSelect={onNewProfile}
        >
          <DragHandle
            disabled
            label={t("New profile")}
            disabledReason={t("This item cannot be reordered")}
          />
          <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md border border-dashed border-primary/40 bg-primary/5 text-primary">
            <Plus className="h-6 w-6" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px] font-semibold text-primary">
              {t("New profile")}
            </div>
            <div className="truncate text-[11px] text-muted-foreground">
              {t("Add a provider profile")}
            </div>
          </div>
        </SelectableItemCard>

        <SelectableItemCard
          active={directActive}
          disabled={busy}
          onSelect={() => onSelect({ kind: "direct" })}
        >
          <DragHandle
            disabled
            label={t("Direct")}
            disabledReason={t("Direct profile is fixed")}
          />
          <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
            <Terminal className="h-6 w-6" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="truncate text-[13px] font-semibold">
                {t("Direct")}
              </span>
              {directIsGlobalDefault && <DefaultBadge />}
            </div>
            <div className="truncate text-[11px] text-muted-foreground">
              {t("Use existing CLI login")}
            </div>
          </div>
          <div
            className="flex shrink-0 flex-wrap justify-end gap-2"
            onClick={(event) => event.stopPropagation()}
          >
            <DirectProfileActionsMenu
              isDefault={directIsGlobalDefault}
              disabled={busy}
              onMakeDefault={() => void onMakeDefault({ kind: "direct" })}
            />
          </div>
        </SelectableItemCard>

        <DragDropProvider onDragEnd={handleProfileDragEnd}>
          {profiles.map((profile, index) => {
            const availability = profileAvailability(profile, agentId, prefs, t);
            return (
              <SortableItem
                key={profile.id}
                id={profile.id}
                index={index}
                disabled={busy}
              >
                {({ dragHandleRef, isDragging }) => {
                  const summary = profileSummary(profile, agentId, prefs, t);
                  const active =
                    availability.launchable &&
                    selected.kind === "profile" &&
                    selected.profileId === profile.id;
                  const globalDefaultForProfile = isGlobalDefaultProfile(
                    prefs,
                    agentId,
                    profile.id,
                  );
                  const connection =
                    isBridgeAgent(agentId)
                      ? resolveProfileConnection(
                          profile,
                          prefs.profileConnections,
                          agentConnectionDef(agentId),
                        )
                      : null;
                  const profileApiOptions =
                    connection?.clientApiTypes.filter((client) => client.native) ?? [];
                  const profileApiSelectValue = profileApiOptions.some(
                    (client) => client.apiType === connection?.selectedApiType,
                  )
                    ? connection?.selectedApiType
                    : profileApiOptions[0]?.apiType;
                  return (
                    <SelectableItemCard
                      active={active}
                      disabled={busy || !availability.launchable}
                      isDragging={isDragging}
                      onSelect={() =>
                        onSelect({ kind: "profile", profileId: profile.id })
                      }
                    >
                      <DragHandle
                        label={t("Reorder {{label}}", { label: profile.label })}
                        disabled={busy}
                        disabledReason={
                          busy ? t("Reordering unavailable while launching") : undefined
                        }
                        dragHandleRef={dragHandleRef}
                      />
                      <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background">
                        <BrandIcon
                          kind="provider"
                          id={profile.provider}
                          label={profile.providerLabel}
                          fallback={profile.providerIcon}
                          framed={false}
                          className="h-9 w-9"
                        />
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                          <span className="truncate text-[13px] font-semibold">
                            {profile.label}
                          </span>
                          {globalDefaultForProfile && <DefaultBadge />}
                        </div>
                        {summary.bridge && (
                          <div className="mt-0.5">
                            <BridgeBadge />
                          </div>
                        )}
                        <div
                          className="truncate text-[11px] text-muted-foreground"
                          title={availability.launchable ? summary.route : availability.reason}
                        >
                          {availability.launchable
                            ? summary.route
                            : availability.reason}
                        </div>
                      </div>
                      <div
                        className="flex shrink-0 flex-wrap items-center justify-end gap-2"
                        onClick={(event) => event.stopPropagation()}
                      >
                        {connection &&
                          connection.agent.supportedApiTypes.length > 1 &&
                          profileApiOptions.length > 0 &&
                          profileApiSelectValue && (
                          <Select
                            value={profileApiSelectValue}
                            disabled={busy}
                            onValueChange={(apiType) => onSelectApiType(profile, apiType)}
                          >
                            <SelectTrigger size="sm" className="h-7 w-[clamp(8rem,20vw,160px)] text-[11px]">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {profileApiOptions.map((option) => (
                                <SelectItem
                                  key={option.apiType}
                                  value={option.apiType}
                                  className="text-xs"
                                >
                                  {apiTypeProtocolDisplayLabel(option.apiType)}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        )}
                        <ProfileActionsMenu
                          profile={profile}
                          bridgeAvailable={isBridgeAgent(agentId)}
                          onMakeDefault={
                            globalDefaultForProfile
                              ? undefined
                              : () =>
                                  void onMakeDefault({
                                    kind: "profile",
                                    profileId: profile.id,
                                  })
                          }
                          makeDefaultDisabled={busy || !availability.launchable}
                          onConnectionSettings={(profile) => {
                            if (isBridgeAgent(agentId)) {
                              onConnectionSettings(profile, agentId);
                            }
                          }}
                          onEditProfile={onEditProfile}
                          onCopyProfile={onCopyProfile}
                          onDeleteProfile={onDeleteProfile}
                        />
                      </div>
                    </SelectableItemCard>
                  );
                }}
              </SortableItem>
            );
          })}
        </DragDropProvider>
      </div>
    </section>
  );
}

export function WorkspacePanel({
  prefs,
  loading,
  onSelect,
  onDelete,
  onReorder,
  onCreate,
  sessionCounts,
  busy,
}: {
  prefs: LauncherPreferences;
  loading: boolean;
  onSelect: (path: string) => void;
  onDelete: (path: string, label: string) => void;
  onReorder: (fromPath: string, toPath: string) => void;
  onCreate: () => void;
  sessionCounts: Record<string, number>;
  busy: boolean;
}) {
  const { t } = useI18n();
  const workspaceOptions = [...prefs.workspaceOptions].sort((a, b) => {
    if (a.isDefault === b.isDefault) return 0;
    return a.isDefault ? -1 : 1;
  });
  const sortableWorkspaces = workspaceOptions.filter(isSortableWorkspace);

  function handleWorkspaceDragEnd(event: DragEndEvent) {
    if (event.canceled || busy) return;
    const { source } = event.operation;
    if (!isSortable(source) || source.initialIndex === source.index) return;
    const from = sortableWorkspaces[source.initialIndex]?.path;
    const to = sortableWorkspaces[source.index]?.path;
    if (from && to) onReorder(from, to);
  }

  function handleWorkspaceRowKeyDown(
    event: KeyboardEvent<HTMLDivElement>,
    workspace: WorkspaceOption,
  ) {
    if (busy || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    onSelect(workspace.path);
  }

  function renderWorkspaceRow(
    workspace: WorkspaceOption,
    dragHandleRef?: Ref<HTMLSpanElement>,
    isDragging = false,
  ) {
    const active = workspace.path === prefs.workspace;
    const sortable = isSortableWorkspace(workspace);
    const canDelete = canDeleteWorkspace(workspace);
    return (
      <div
        role="button"
        key={workspace.path}
        tabIndex={busy ? -1 : 0}
        className={`group flex w-full items-center gap-2 px-3 py-2 text-left transition-colors ${
          active
            ? "bg-primary/10 text-primary"
            : "text-foreground hover:bg-accent/50"
        } ${busy ? "cursor-not-allowed opacity-60" : "cursor-pointer"} ${
          isDragging ? "opacity-55" : ""
        }`}
        aria-disabled={busy}
        data-dragging={isDragging ? "true" : undefined}
        onClick={() => {
          if (!busy) onSelect(workspace.path);
        }}
        onKeyDown={(event) => handleWorkspaceRowKeyDown(event, workspace)}
      >
        <DragHandle
          disabled={!sortable || busy}
          label={t("Reorder {{label}}", { label: workspace.label })}
          disabledReason={
            !sortable
              ? workspace.isDefault
                ? t("Default workspace is fixed")
                : t("This item cannot be reordered")
              : t("Reordering unavailable while launching")
          }
          dragHandleRef={dragHandleRef}
        />
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
          <FolderOpen className="h-4 w-4" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="truncate text-[13px] font-semibold">
              {workspace.label}
            </span>
            {workspace.isDefault && <DefaultBadge />}
          </span>
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="block truncate text-[11px] text-muted-foreground">
                {workspace.detail}
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-[min(520px,calc(100vw-2rem))] break-all text-left">
              {workspace.path}
            </TooltipContent>
          </Tooltip>
        </span>
        <span className="w-8 shrink-0 text-right font-mono text-[11px] text-muted-foreground">
          {sessionCounts[workspace.path] ?? ""}
        </span>
        {active ? (
          <Check className="h-4 w-4 shrink-0 text-primary" />
        ) : (
          <span className="h-4 w-4 shrink-0" aria-hidden="true" />
        )}
        <span
          className="flex shrink-0 flex-wrap items-center justify-end gap-2"
          onClick={(event) => event.stopPropagation()}
        >
          {canDelete ? (
            <WorkspaceActionsMenu
              workspace={workspace}
              onDelete={(target) => onDelete(target.path, target.label)}
            />
          ) : (
            <DisabledMoreButton
              reason={
                workspace.isDefault
                  ? t("Default workspace cannot be edited or deleted")
                  : t("No actions available")
              }
            />
          )}
        </span>
      </div>
    );
  }

  return (
    <SelectorPanelShell
      title={t("Switch workspace")}
      footer={
        <button
          type="button"
          disabled={busy}
          className="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-primary transition-colors hover:bg-primary/5 disabled:cursor-not-allowed disabled:opacity-60"
          onClick={onCreate}
        >
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-dashed border-primary/40 bg-primary/5">
            <Plus className="h-4 w-4" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-semibold">
              {t("New workspace...")}
            </span>
            <span className="block truncate text-[11px] text-muted-foreground">
              {t("Choose folder...")}
            </span>
          </span>
        </button>
      }
    >
      {loading && workspaceOptions.length === 0 && (
        <p className="px-3 py-2 text-xs text-muted-foreground">
          {t("Loading…")}
        </p>
      )}
      <DragDropProvider onDragEnd={handleWorkspaceDragEnd}>
        <div className="min-h-0 flex-1 divide-y divide-border/60 overflow-y-auto">
          {workspaceOptions.map((workspace) => {
            if (!isSortableWorkspace(workspace)) {
              return renderWorkspaceRow(workspace);
            }
            const index = sortableWorkspaces.findIndex(
              (sortable) => sortable.path === workspace.path,
            );
            return (
              <SortableItem
                key={workspace.path}
                id={workspace.path}
                index={index}
                disabled={busy}
              >
                {({ dragHandleRef, isDragging }) =>
                  renderWorkspaceRow(workspace, dragHandleRef, isDragging)
                }
              </SortableItem>
            );
          })}
        </div>
      </DragDropProvider>
    </SelectorPanelShell>
  );
}

export function SessionPanel({
  sessions,
  selected,
  archiveFilterAvailable,
  resumeSupported,
  unsupportedReason,
  showArchived,
  onShowArchivedChange,
  onSelect,
}: {
  sessions: LaunchSessionSummary[];
  selected: SessionChoice;
  archiveFilterAvailable: boolean;
  resumeSupported: boolean;
  unsupportedReason: string;
  showArchived: boolean;
  onShowArchivedChange: (show: boolean) => void;
  onSelect: (choice: SessionChoice) => void;
}) {
  const { t } = useI18n();
  if (!resumeSupported) {
    return (
      <p className="rounded-md border border-dashed border-border px-3 py-2 text-xs text-muted-foreground">
        {unsupportedReason}
      </p>
    );
  }
  return (
    <SelectorPanelShell
      title={t("Switch session")}
      headerExtra={
        archiveFilterAvailable ? (
          <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
            <Archive className="h-3.5 w-3.5" />
            <span>{t("Show archived")}</span>
            <Switch
              checked={showArchived}
              onCheckedChange={onShowArchivedChange}
              aria-label={t("Show archived")}
            />
          </label>
        ) : undefined
      }
      footer={
        <button
          type="button"
          className={`flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors ${
            selected === null
              ? "bg-primary/10 text-primary"
              : "text-foreground hover:bg-primary/5"
          }`}
          onClick={() => onSelect(null)}
        >
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
            <Plus className="h-4 w-4" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-semibold">
              {t("New session")}
            </span>
            <span className="block truncate text-[11px] text-muted-foreground">
              {t("Quick Launch will start a new session")}
            </span>
          </span>
          {selected === null ? (
            <Check className="h-4 w-4 shrink-0 text-primary" />
          ) : (
            <span className="h-4 w-4 shrink-0" aria-hidden="true" />
          )}
        </button>
      }
    >
      <div className="min-h-0 flex-1 divide-y divide-border/60 overflow-y-auto">
        {sessions.length === 0 && (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {t("No session in this workspace")}
          </p>
        )}
        {sessions.map((session) => {
          const isLast = session === sessions[0];
          const active =
            selected?.kind === "session" &&
            selected.sessionId === session.sessionId;
          return (
            <button
              type="button"
              key={`${session.sessionId}:${session.archived ? "archived" : "active"}`}
              className={`flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors ${
                active
                  ? "bg-primary/10 text-primary"
                  : "text-foreground hover:bg-accent/50"
              }`}
              onClick={() =>
                onSelect({ kind: "session", sessionId: session.sessionId })
              }
            >
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
                <MessageCircle className="h-4 w-4" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="truncate text-[13px] font-semibold">
                    {session.title}
                  </span>
                  {session.archived && (
                    <span className="inline-flex shrink-0 items-center gap-1 rounded border border-amber-500/25 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                      <Archive className="h-3 w-3" />
                      {t("Archived")}
                    </span>
                  )}
                  {isLast && (
                    <span className="inline-flex shrink-0 items-center gap-1 rounded border border-primary/25 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                      <History className="h-3 w-3" />
                      {t("Last session")}
                    </span>
                  )}
                </span>
                <span className="block truncate font-mono text-[11px] text-muted-foreground">
                  {session.shortId} · {relativeTime(session.updatedAt, t)}
                </span>
              </span>
              {active ? (
                <Check className="h-4 w-4 shrink-0 text-primary" />
              ) : (
                <span className="h-4 w-4 shrink-0" aria-hidden="true" />
              )}
            </button>
          );
        })}
      </div>
    </SelectorPanelShell>
  );
}
