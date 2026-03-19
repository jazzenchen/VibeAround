import { useState, useEffect } from "react";
import {
  Globe, Bot, MessageSquare, Terminal, X, RefreshCw, ExternalLink, Server, Wifi, WifiOff,
} from "lucide-react";
import { useServices, type ServiceInfo } from "./hooks/useServices";
import Onboarding from "./Onboarding";

function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

function StatusDot({ status }: { status: string }) {
  const color =
    status === "running"
      ? "bg-emerald-500"
      : status === "failed"
        ? "bg-red-500"
        : "bg-zinc-400";
  return (
    <span className={`inline-block w-2 h-2 rounded-full ${color}`} />
  );
}

function ServiceRow({
  service,
  onKill,
}: {
  service: ServiceInfo;
  onKill: () => void;
}) {
  return (
    <div className="flex items-center justify-between py-2 px-3 rounded-md hover:bg-accent/50 transition-colors group">
      <div className="flex items-center gap-2.5 min-w-0">
        <StatusDot status={service.status} />
        <span className="text-sm font-medium truncate">{service.name}</span>
        {service.url && (
          <a
            href={service.url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-primary hover:underline flex items-center gap-0.5 shrink-0"
          >
            {service.url.replace(/^https?:\/\//, "")}
            <ExternalLink className="w-3 h-3" />
          </a>
        )}
        {service.role && (
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground bg-muted px-1.5 py-0.5 rounded shrink-0">
            {service.role}
          </span>
        )}
      </div>
      <div className="flex items-center gap-3 shrink-0">
        <span className="text-xs text-muted-foreground capitalize">
          {service.status}
        </span>
        {service.status === "running" && (
          <span className="text-xs text-muted-foreground tabular-nums">
            {formatUptime(service.uptime_secs)}
          </span>
        )}
        {service.status_detail && service.status !== "running" && (
          <span className="text-xs text-destructive truncate max-w-[120px]" title={service.status_detail}>
            {service.status_detail}
          </span>
        )}
        {service.status === "running" && (
          <button
            onClick={onKill}
            className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-destructive/10 text-destructive"
            title="Kill"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
    </div>
  );
}

function Section({
  icon,
  title,
  children,
  badge,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
  badge?: string | number;
}) {
  return (
    <div className="border border-border rounded-lg overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 bg-muted/40 border-b border-border">
        {icon}
        <span className="text-sm font-semibold">{title}</span>
        {badge !== undefined && (
          <span className="ml-auto text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded-full tabular-nums">
            {badge}
          </span>
        )}
      </div>
      <div className="divide-y divide-border/50">{children}</div>
    </div>
  );
}

function App() {
  // Route: if path is /onboarding, show the wizard; otherwise show dashboard.
  const [route, setRoute] = useState(() => window.location.pathname);

  useEffect(() => {
    const onPop = () => setRoute(window.location.pathname);
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  if (route === "/onboarding") {
    return <Onboarding />;
  }

  return <Dashboard />;
}

function Dashboard() {
  const { data, error, loading, connected, refresh, killService } = useServices();

  if (loading && !data) {
    return (
      <div className="flex items-center justify-center h-full">
        <RefreshCw className="w-5 h-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (error && !data) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <p className="text-sm text-destructive">{error}</p>
        <button
          onClick={refresh}
          className="text-xs text-primary hover:underline flex items-center gap-1"
        >
          <RefreshCw className="w-3 h-3" /> Retry
        </button>
      </div>
    );
  }

  if (!data) return null;

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <header className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <div className="flex items-center gap-2">
          <Server className="w-4 h-4 text-primary" />
          <span className="font-semibold text-sm">VibeAround</span>
        </div>
        <div className="flex items-center gap-3">
          {connected ? (
            <span className="flex items-center gap-1 text-xs text-emerald-600">
              <Wifi className="w-3 h-3" /> Live
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              <WifiOff className="w-3 h-3" /> Polling
            </span>
          )}
          <span className="text-xs text-muted-foreground tabular-nums">
            port {data.server.port}
          </span>
          <button
            onClick={refresh}
            className="p-1 rounded hover:bg-accent transition-colors"
            title="Refresh"
          >
            <RefreshCw className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </header>

      {/* Error banner */}
      {error && (
        <div className="px-4 py-1.5 bg-destructive/10 text-destructive text-xs">
          {error}
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {/* Tunnel */}
        <Section
          icon={<Globe className="w-4 h-4 text-primary" />}
          title="Tunnel"
          badge={data.tunnel.length}
        >
          {data.tunnel.length === 0 ? (
            <p className="text-xs text-muted-foreground px-3 py-2">
              No tunnel running
            </p>
          ) : (
            data.tunnel.map((s) => (
              <ServiceRow
                key={s.id}
                service={s}
                onKill={() => killService("tunnel", s.id)}
              />
            ))
          )}
        </Section>

        {/* Agents */}
        <Section
          icon={<Bot className="w-4 h-4 text-primary" />}
          title="Agents"
          badge={data.agents.length}
        >
          {data.agents.length === 0 ? (
            <p className="text-xs text-muted-foreground px-3 py-2">
              No agents running
            </p>
          ) : (
            data.agents.map((s) => (
              <ServiceRow
                key={s.id}
                service={s}
                onKill={() => killService("agents", s.id)}
              />
            ))
          )}
        </Section>

        {/* IM Bots */}
        <Section
          icon={<MessageSquare className="w-4 h-4 text-primary" />}
          title="IM Bots"
          badge={data.im_bots.length}
        >
          {data.im_bots.length === 0 ? (
            <p className="text-xs text-muted-foreground px-3 py-2">
              No IM bots running
            </p>
          ) : (
            data.im_bots.map((s) => (
              <ServiceRow
                key={s.id}
                service={s}
                onKill={() => killService("im_bots", s.id)}
              />
            ))
          )}
        </Section>

        {/* PTY Sessions */}
        <Section
          icon={<Terminal className="w-4 h-4 text-primary" />}
          title="PTY Sessions"
          badge={data.pty_session_count}
        >
          <div className="flex items-center justify-between px-3 py-2">
            <span className="text-sm text-muted-foreground">
              {data.pty_session_count} active session{data.pty_session_count !== 1 ? "s" : ""}
            </span>
            <a
              href={`http://127.0.0.1:${data.server.port}`}
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-primary hover:underline flex items-center gap-1"
            >
              Open Web Dashboard <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        </Section>
      </div>
    </div>
  );
}

export default App;
