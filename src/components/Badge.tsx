import type { ReactNode } from "react";

const TONES = {
  neutral: "border-line text-muted",
  wave: "border-wave/40 text-wave",
  caution: "border-caution/40 text-caution",
} as const;

export function Badge({
  children,
  tone = "neutral",
  title,
}: {
  children: ReactNode;
  tone?: keyof typeof TONES;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={`inline-flex shrink-0 items-center rounded border px-1.5 py-0.5 text-[11px] leading-none ${TONES[tone]}`}
    >
      {children}
    </span>
  );
}
