"use client";

import { useI18n } from "@va/i18n";
import { Button } from "@/components/ui/button";
import { asRecord, stringField } from "./chatFrameUtils";
import type { PendingPermission } from "./chatTypes";

type PermissionOptionView = {
  optionId: string;
  name: string;
  kind?: string;
};

export interface PendingPermissionsProps {
  permissions: PendingPermission[];
  onRespond: (requestId: string, optionId: string) => void;
  onCancel: (requestId: string) => void;
}

function permissionTitle(request: unknown) {
  const root = asRecord(request);
  const toolCall = asRecord(root?.toolCall);
  return (
    stringField(toolCall, "title") ??
    stringField(toolCall, "kind") ??
    "Permission requested"
  );
}

function permissionOptions(request: unknown): PermissionOptionView[] {
  const root = asRecord(request);
  const options = root && Array.isArray(root.options) ? root.options : [];
  return options.flatMap((option) => {
    const record = asRecord(option);
    const optionId = stringField(record, "optionId");
    return optionId
      ? [
          {
            optionId,
            name: stringField(record, "name") ?? optionId,
            kind: stringField(record, "kind"),
          },
        ]
      : [];
  });
}

function permissionButtonClass(kind?: string) {
  const base =
    "h-auto min-h-6 max-w-full shrink justify-start whitespace-normal break-words px-2.5 py-1.5 text-left leading-snug [overflow-wrap:anywhere]";
  if (kind?.startsWith("reject")) {
    return `${base} border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15`;
  }
  return `${base} border-primary/30 bg-primary/10 text-primary hover:bg-primary/15`;
}

export function PendingPermissions({
  permissions,
  onRespond,
  onCancel,
}: PendingPermissionsProps) {
  const { t } = useI18n();

  if (!permissions.length) return null;

  return (
    <div className="bg-background px-4 py-3">
      <div className="mx-auto flex max-w-3xl flex-col gap-2">
        {permissions.map((permission) => {
          const options = permissionOptions(permission.request);
          return (
            <div
              key={permission.requestId}
              className="rounded-md border border-border/70 bg-muted/25 px-3 py-3"
            >
              <div className="flex min-w-0 flex-col gap-3">
                <div className="min-w-0">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    {t("Permission request")}
                  </div>
                  <div className="break-words text-sm font-medium text-foreground [overflow-wrap:anywhere]">
                    {permissionTitle(permission.request)}
                  </div>
                </div>
                <div className="flex min-w-0 max-w-full flex-wrap items-start gap-2">
                  {options.map((option) => (
                    <Button
                      key={option.optionId}
                      type="button"
                      variant="outline"
                      size="xs"
                      onClick={() => onRespond(permission.requestId, option.optionId)}
                      className={permissionButtonClass(option.kind)}
                    >
                      {option.name}
                    </Button>
                  ))}
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    onClick={() => onCancel(permission.requestId)}
                    className="text-muted-foreground"
                  >
                    {t("Cancel")}
                  </Button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
