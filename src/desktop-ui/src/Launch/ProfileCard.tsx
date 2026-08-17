import { useState } from "react";
import type { Ref } from "react";
import {
  GripVertical,
  MoreVertical,
  Pencil,
  Plug,
  SlidersHorizontal,
  Star,
  Trash2,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Badge } from "@/components/ui/badge";
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
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  CONNECTION_AGENTS,
  apiTypeProtocolLabel,
  resolveProfileConnection,
} from "./connections";
import type { ProfileConnections, ProfileSummary } from "./types";
import { apiTypeBadge, apiTypeShort } from "./types";

interface Props {
  profile: ProfileSummary;
  onLaunch: (launchTarget: string) => Promise<void>;
  onSetDefault: (launchTarget: string) => Promise<void>;
  onEdit: () => void;
  onDelete: () => Promise<void>;
  onConnectionSettings: () => void;
  defaultAgent?: string;
  defaultProfiles?: Record<string, string>;
  profileConnections?: ProfileConnections;
  dragHandleRef?: Ref<HTMLDivElement>;
  dragHandleDisabled?: boolean;
  isDragging?: boolean;
}

export function ProfileCard({
  profile,
  onLaunch,
  onSetDefault,
  onEdit,
  onDelete,
  onConnectionSettings,
  defaultAgent,
  defaultProfiles = {},
  profileConnections,
  dragHandleRef,
  dragHandleDisabled = false,
  isDragging = false,
}: Props) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [defaultBusy, setDefaultBusy] = useState<string | null>(null);
  const connections = CONNECTION_AGENTS.map((agent) =>
    resolveProfileConnection(profile, profileConnections, agent),
  );
  const isDefaultProfile = connections.some(
    (connection) =>
      connection.status !== "unsupported" &&
      defaultAgent === connection.agent.id &&
      defaultProfiles[connection.agent.id] === profile.id,
  );

  async function handleLaunch(launchTarget: string) {
    setBusy(true);
    try {
      await onLaunch(launchTarget);
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (
      !window.confirm(
        t('Delete profile "{{label}}"?', { label: profile.label }),
      )
    )
      return;
    setBusy(true);
    try {
      await onDelete();
    } finally {
      setBusy(false);
    }
  }

  async function handleSetDefault(launchTarget: string) {
    setBusy(true);
    try {
      await onSetDefault(launchTarget);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className={`border rounded-md p-2.5 flex flex-col gap-1.5 transition-colors ${
        isDefaultProfile
          ? "border-emerald-500/70 bg-emerald-500/5 hover:border-emerald-500"
          : "border-border bg-card hover:border-primary/40"
      } ${isDragging ? "opacity-55" : ""}`}
    >
      <div className="flex items-start gap-2">
        {dragHandleRef && (
          <div
            ref={dragHandleRef}
            role="button"
            tabIndex={dragHandleDisabled ? -1 : 0}
            aria-label={t("Reorder {{label}}", { label: profile.label })}
            aria-disabled={dragHandleDisabled}
            className={`mt-0.5 h-7 w-5 shrink-0 rounded text-muted-foreground/60 hover:bg-accent hover:text-foreground inline-flex items-center justify-center select-none ${
              dragHandleDisabled
                ? "cursor-not-allowed opacity-40"
                : "cursor-grab active:cursor-grabbing"
            }`}
          >
            <GripVertical className="w-3.5 h-3.5" />
          </div>
        )}
        <BrandIcon
          kind="provider"
          id={profile.provider}
          label={profile.providerLabel}
          fallback={profile.providerIcon}
          className="h-7 w-7"
        />
        <div className="flex-1 min-w-0">
          <div className="text-[13px] font-medium truncate">
            {profile.label}
          </div>
          <div className="text-[11px] text-muted-foreground truncate">
            {profile.providerLabel}
          </div>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="shrink-0 text-muted-foreground"
              aria-label={t("More")}
            >
              <MoreVertical className="w-3.5 h-3.5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            <DropdownMenuItem
              className="text-xs"
              onSelect={onConnectionSettings}
            >
              <SlidersHorizontal className="w-3 h-3" />{" "}
              {t("Connection settings")}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem className="text-xs" onSelect={onEdit}>
              <Pencil className="w-3 h-3" /> {t("Edit")}
            </DropdownMenuItem>
            <DropdownMenuItem
              className="text-xs"
              variant="destructive"
              onSelect={() => {
                void handleDelete();
              }}
            >
              <Trash2 className="w-3 h-3" /> {t("Delete")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <TooltipProvider>
        <div className="flex flex-wrap gap-1.5 mt-1">
          {connections.map((connection) => {
            const target = {
              id: connection.agent.id,
              label: connection.agent.label,
              apiType: connection.selectedApiType,
            };
            const isDefault =
              connection.status !== "unsupported" &&
              defaultAgent === target.id &&
              defaultProfiles[target.id] === profile.id;
            const statusText =
              connection.status === "via_bridge"
                ? t("via API bridge")
                : connection.status === "native"
                  ? apiTypeBadge(target.apiType)
                  : t("unsupported");
            const title =
              connection.status === "via_bridge" &&
              connection.selected.targetApiType
                ? t(
                    "{{agent}} connects through the API bridge to {{provider}} {{apiType}}",
                    {
                      agent: target.label,
                      provider: profile.providerLabel,
                      apiType: apiTypeProtocolLabel(
                        connection.selected.targetApiType,
                      ),
                    },
                  )
                : connection.selected.native
                  ? t("Launch {{agent}} via {{apiType}}", {
                      agent: target.label,
                      apiType: apiTypeShort(target.apiType),
                    })
                  : t("{{agent}} is unsupported for this profile", {
                      agent: target.label,
                    });
            return (
              <span
                key={target.id}
                className={`inline-flex h-7 overflow-hidden rounded-md ${
                  connection.status === "unsupported"
                    ? "bg-muted text-muted-foreground"
                    : "bg-primary/10 text-primary"
                }`}
              >
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      onClick={() => {
                        if (connection.status !== "unsupported") {
                          void handleLaunch(target.id);
                        } else {
                          onConnectionSettings();
                        }
                      }}
                      disabled={busy}
                      className={`h-7 rounded-none bg-transparent px-2 font-mono text-[11px] ${
                        connection.status === "unsupported"
                          ? "text-muted-foreground hover:bg-muted hover:text-muted-foreground"
                          : "text-primary hover:bg-primary/15 hover:text-primary"
                      } ${isDefault ? "" : "pr-1.5"}`}
                    >
                      <BrandIcon
                        kind="cli"
                        id={target.id}
                        label={target.label}
                        framed={false}
                        className="h-3.5 w-3.5"
                      />
                      <span>{target.label}</span>
                      <Badge
                        className={`border-0 bg-transparent p-0 text-[11px] ${
                          connection.status === "unsupported"
                            ? "text-muted-foreground/70"
                            : "text-primary/55"
                        }`}
                      >
                        · {statusText}
                      </Badge>
                      {connection.status === "via_bridge" && (
                        <Plug className="w-3 h-3" />
                      )}
                      {isDefault && <Star className="w-3 h-3 fill-current" />}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent
                    side="bottom"
                    sideOffset={6}
                    className="max-w-72 whitespace-pre-line text-left"
                  >
                    {title}
                  </TooltipContent>
                </Tooltip>
                {connection.status !== "unsupported" && !isDefault && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    disabled={busy || defaultBusy === target.id}
                    onClick={async () => {
                      setDefaultBusy(target.id);
                      try {
                        await handleSetDefault(target.id);
                      } finally {
                        setDefaultBusy(null);
                      }
                    }}
                    title={t(
                      "Use {{agent}} with {{profile}} as Quick Launch default",
                      {
                        agent: target.label,
                        profile: profile.label,
                      },
                    )}
                    className="h-7 w-6 rounded-none border-l border-primary/15 bg-transparent text-primary/60 hover:bg-primary/15 hover:text-primary"
                  >
                    <Star className="w-3 h-3" />
                  </Button>
                )}
              </span>
            );
          })}
        </div>
      </TooltipProvider>
    </div>
  );
}
