import { useState } from "react";
import { FileText, Plus, Trash2 } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  apiTypeProtocolLabel,
  apiTypeRouteLabel,
} from "./connections";
import type { ConnectionAgentId } from "./types";

export interface HeaderSetting {
  agentId: ConnectionAgentId;
  agentLabel: string;
  clientApiType: string;
  targetApiType: string;
  defaultHeaders: Record<string, string>;
  headers: Record<string, string>;
}

type HeaderRow = {
  id: string;
  name: string;
  value: string;
};

let nextHeaderRowId = 0;

export function HeaderSummaryButton({
  defaultHeaders,
  headers,
  disabled,
  onClick,
}: {
  defaultHeaders: Record<string, string>;
  headers: Record<string, string>;
  disabled?: boolean;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const defaultCount = Object.keys(defaultHeaders).length;
  const customCount = countHeaders(headers);

  return (
    <Button
      type="button"
      variant="outline"
      size="xs"
      disabled={disabled}
      onClick={onClick}
      className="h-7 w-fit gap-1.5 rounded-md border-primary/40 bg-primary/5 px-2.5 text-[11px] font-medium text-primary shadow-xs hover:bg-primary/10 hover:text-primary"
    >
      <FileText className="h-3 w-3" />
      {t("Headers")}
      <span className="text-muted-foreground">
        {defaultCount + customCount}
        {customCount > 0 ? ` · +${customCount}` : ""}
      </span>
    </Button>
  );
}

export function HeaderSettingDialog({
  setting,
  onSave,
  onClose,
}: {
  setting: HeaderSetting;
  onSave: (headers: Record<string, string>) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState(() => headerMapToRows(setting.headers));

  function handleSave() {
    onSave(rowsToHeaderMap(rows));
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(620px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(620px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 px-6 pt-6 pr-12">
          <DialogTitle>{t("Headers")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("Configure API bridge headers.")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-6 pb-3 [scrollbar-gutter:stable]">
          <div className="font-mono text-[11px] leading-5 text-primary">
            {setting.agentLabel} ·{" "}
            {apiTypeProtocolDisplayLabel(setting.clientApiType)} -&gt;{" "}
            {apiTypeRouteDisplayLabel(setting.targetApiType)}
          </div>
          <BridgeHeadersField
            defaultHeaders={setting.defaultHeaders}
            rows={rows}
            onChange={setRows}
          />
        </div>
        <DialogFooter className="shrink-0 border-t border-border px-6 py-4">
          <Button type="button" variant="outline" size="sm" onClick={onClose}>
            {t("Cancel")}
          </Button>
          <Button type="button" size="sm" onClick={handleSave}>
            {t("Save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function BridgeHeadersField({
  defaultHeaders,
  rows,
  onChange,
}: {
  defaultHeaders: Record<string, string>;
  rows: HeaderRow[];
  onChange: (v: HeaderRow[]) => void;
}) {
  const { t } = useI18n();
  const defaultEntries = Object.entries(defaultHeaders);

  function updateName(id: string, nextName: string) {
    onChange(rows.map((row) => (row.id === id ? { ...row, name: nextName } : row)));
  }

  function updateValue(id: string, value: string) {
    onChange(rows.map((row) => (row.id === id ? { ...row, value } : row)));
  }

  function removeHeader(id: string) {
    onChange(rows.filter((row) => row.id !== id));
  }

  function addHeader() {
    onChange([...rows, newHeaderRow("", "")]);
  }

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <div className="text-[11px] font-medium text-muted-foreground">
          {t("Headers")}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          onClick={addHeader}
          aria-label={t("Add header")}
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>
      {defaultEntries.length > 0 && (
        <div className="space-y-1.5">
          <div className="text-[10px] font-medium text-muted-foreground">
            {t("Default headers")}
          </div>
          {defaultEntries.map(([name, value]) => (
            <div
              key={`default-${name}`}
              className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem] gap-1.5"
            >
              <Input
                type="text"
                value={name}
                disabled
                readOnly
                className="h-7 w-full font-mono text-xs"
              />
              <Input
                type="text"
                value={value}
                disabled
                readOnly
                className="h-7 w-full font-mono text-xs"
              />
              <div aria-hidden className="h-7 w-8" />
            </div>
          ))}
        </div>
      )}
      {rows.length > 0 && (
        <div className="space-y-1.5">
          <div className="text-[10px] font-medium text-muted-foreground">
            {t("Append headers")}
          </div>
          {rows.map((row) => (
            <div
              key={row.id}
              className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem] gap-1.5"
            >
              <Input
                type="text"
                value={row.name}
                onChange={(event) => updateName(row.id, event.currentTarget.value)}
                placeholder="HTTP-Referer"
                className="h-7 w-full font-mono text-xs"
              />
              <Input
                type="text"
                value={row.value}
                onChange={(event) => updateValue(row.id, event.currentTarget.value)}
                placeholder="Bearer $apiKey"
                className="h-7 w-full font-mono text-xs"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => removeHeader(row.id)}
                aria-label={t("Remove header")}
                className="h-7 w-8"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function headerMapToRows(headers: Record<string, string>): HeaderRow[] {
  return Object.entries(headers).map(([name, value]) => newHeaderRow(name, value));
}

function rowsToHeaderMap(rows: HeaderRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of rows) {
    const name = row.name.trim();
    if (name) out[name] = row.value;
  }
  return out;
}

function countHeaders(headers: Record<string, string>): number {
  return Object.keys(headers).filter((name) => name.trim()).length;
}

function newHeaderRow(name: string, value: string): HeaderRow {
  nextHeaderRowId += 1;
  return {
    id: `header-row-${nextHeaderRowId}`,
    name,
    value,
  };
}

function apiTypeProtocolDisplayLabel(apiType: string): string {
  return apiTypeProtocolLabel(apiType);
}

function apiTypeRouteDisplayLabel(apiType: string): string {
  return apiTypeRouteLabel(apiType);
}
