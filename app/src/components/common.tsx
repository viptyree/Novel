import { ReactNode } from "react";
import { useT } from "../i18n";

export function Spinner() {
  const t = useT();
  return <span className="spinner" role="status" aria-label={t("ui.loading")} />;
}

export function LogBox({ log }: { log: string }) {
  const t = useT();
  if (!log) return null;
  return <details className="activity-log"><summary>{t("ui.logs")}</summary><pre className="logbox">{log}</pre></details>;
}

export function Banner({
  kind,
  children,
}: {
  kind: "ok" | "err" | "info";
  children: ReactNode;
}) {
  return <div role={kind === "err" ? "alert" : "status"} className={`banner banner-${kind}`}>{children}</div>;
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </label>
  );
}
