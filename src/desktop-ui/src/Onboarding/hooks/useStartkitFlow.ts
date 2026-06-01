import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Settings,
  StartkitChoices,
  StartkitCompleteEvent,
  StartkitItemReport,
  StartkitPlan,
  StartkitProgressEvent,
  StartkitScanReport,
} from "../types";

interface UseStartkitFlowResult {
  plan: StartkitPlan | null;
  reports: StartkitItemReport[];
  scanning: boolean;
  running: boolean;
  complete: boolean;
  finalStatus: string | null;
  error: string | null;
  reportById: Map<string, StartkitItemReport>;
  refreshPlan: (choices: StartkitChoices) => Promise<void>;
  scan: (settings: Settings, choices: StartkitChoices) => Promise<void>;
  start: (settings: Settings, choices: StartkitChoices) => Promise<void>;
  cancel: () => Promise<void>;
  finish: () => Promise<void>;
}

export function useStartkitFlow(): UseStartkitFlowResult {
  const [plan, setPlan] = useState<StartkitPlan | null>(null);
  const [reports, setReports] = useState<StartkitItemReport[]>([]);
  const [scanning, setScanning] = useState(false);
  const [running, setRunning] = useState(false);
  const [complete, setComplete] = useState(false);
  const [finalStatus, setFinalStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const unlistenRefs = useRef<UnlistenFn[]>([]);

  const reportById = useMemo(
    () => new Map(reports.map((report) => [report.id, report])),
    [reports],
  );

  const refreshPlan = useCallback(async (choices: StartkitChoices) => {
    try {
      const nextPlan = await invoke<StartkitPlan>("startkit_plan", { choices });
      setPlan(nextPlan);
      setReports((previous) =>
        nextPlan.items.map(
          (item) =>
            previous.find((report) => report.id === item.id) ?? {
              id: item.id,
              label: item.label,
              group: item.group,
              category: item.category,
              status: "pending",
              severity: item.severity,
              actions: [],
              secret: item.secret,
              settingsKey: item.settingsKey,
            },
        ),
      );
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const scan = useCallback(async (settings: Settings, choices: StartkitChoices) => {
    setScanning(true);
    setError(null);
    try {
      const report = await invoke<StartkitScanReport>("startkit_scan", {
        settings,
        choices,
      });
      setPlan(report.plan);
      setReports(report.reports);
      setComplete(false);
      setFinalStatus(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setScanning(false);
    }
  }, []);

  const start = useCallback(async (settings: Settings, choices: StartkitChoices) => {
    setRunning(true);
    setComplete(false);
    setFinalStatus(null);
    setError(null);

    for (const unlisten of unlistenRefs.current) unlisten();
    unlistenRefs.current = [];

    try {
      const unlistenProgress = await listen<StartkitProgressEvent>(
        "startkit-progress",
        (event) => {
          const payload = event.payload;
          setReports((previous) => {
            const next = [...previous];
            const index = next.findIndex((report) => report.id === payload.id);
            const base: StartkitItemReport =
              payload.report ??
              (index >= 0
                ? {
                    ...next[index],
                    status: payload.status,
                    message: payload.message,
                  }
                : {
                    id: payload.id,
                    label: payload.label,
                    group: "startkit",
                    category: "startkit",
                    status: payload.status,
                    message: payload.message,
                    actions: [],
                    secret: false,
                  });
            if (index >= 0) next[index] = base;
            else next.push(base);
            return next;
          });
        },
      );

      const unlistenComplete = await listen<StartkitCompleteEvent>(
        "startkit-complete",
        (event) => {
          setFinalStatus(event.payload.status);
          setComplete(true);
          setRunning(false);
        },
      );

      unlistenRefs.current = [unlistenProgress, unlistenComplete];
      await invoke("start_startkit_install", { settings, choices });
    } catch (err) {
      setError(String(err));
      setRunning(false);
    }
  }, []);

  const cancel = useCallback(async () => {
    await invoke("cancel_startkit_install");
    setRunning(false);
    setComplete(true);
    setFinalStatus("cancelled");
  }, []);

  const finish = useCallback(async () => {
    for (const unlisten of unlistenRefs.current) unlisten();
    unlistenRefs.current = [];
    await invoke("finish_onboarding");
    window.location.replace("/");
  }, []);

  return {
    plan,
    reports,
    scanning,
    running,
    complete,
    finalStatus,
    error,
    reportById,
    refreshPlan,
    scan,
    start,
    cancel,
    finish,
  };
}

