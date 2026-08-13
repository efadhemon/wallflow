import { ChevronDown, ChevronUp } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";
import { cn } from "../lib/utils";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "sm" | "md" | "lg" | "icon";

const VARIANTS: Record<ButtonVariant, string> = {
  primary:
    "bg-brand text-on-brand hover:bg-brand-hover active:bg-brand shadow-sm disabled:bg-line disabled:text-ink-muted disabled:shadow-none",
  secondary: "bg-surface text-ink border border-line hover:bg-surface-muted disabled:text-ink-muted",
  ghost: "text-ink-muted hover:bg-surface-muted hover:text-ink",
  danger: "bg-danger-soft text-danger hover:bg-danger/15",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "h-8 px-3 text-xs gap-1.5",
  md: "h-9.5 px-4 text-sm gap-2",
  lg: "h-12 px-6 text-base gap-2.5",
  icon: "h-9.5 w-9.5",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({ variant = "secondary", size = "md", className, ...props }: ButtonProps) {
  return (
    <button
      className={cn(
        "focus-ring inline-flex shrink-0 items-center justify-center rounded-lg font-medium transition disabled:cursor-not-allowed disabled:opacity-70",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...props}
    />
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cn("rounded-xl border border-line bg-surface shadow-[0_1px_2px_rgb(0_0_0/0.04)]", className)}>
      {children}
    </div>
  );
}

export function SectionTitle({ children }: { children: ReactNode }) {
  return <h2 className="text-xs font-semibold tracking-wider text-ink-muted uppercase">{children}</h2>;
}

/** Label + description + control, the building block of the settings screen. */
export function Field({
  label,
  hint,
  htmlFor,
  control,
}: {
  label: string;
  hint?: string;
  htmlFor?: string;
  control: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3.5">
      <div className="min-w-0">
        <label htmlFor={htmlFor} className="block text-sm font-medium text-ink">
          {label}
        </label>
        {hint && <p className="mt-0.5 text-xs text-ink-muted">{hint}</p>}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  id,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  id?: string;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "focus-ring relative h-6 w-11 shrink-0 rounded-full transition disabled:cursor-not-allowed disabled:opacity-50",
        checked ? "bg-brand" : "bg-line",
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform",
          checked && "translate-x-5",
        )}
      />
    </button>
  );
}

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className="relative inline-flex items-center">
      <select
        className={cn(
          "focus-ring h-9.5 cursor-pointer rounded-lg border border-line bg-surface py-0 pr-9 pl-3 text-sm text-ink transition hover:bg-surface-muted disabled:cursor-not-allowed disabled:text-ink-muted",
          className,
        )}
        {...props}
      />
      <ChevronDown size={15} aria-hidden className="pointer-events-none absolute right-3 text-ink-muted" />
    </div>
  );
}

export function NumberInput({
  value,
  onChange,
  min = 1,
  max = 9999,
  id,
  label,
}: {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  id?: string;
  label: string;
}) {
  const clamp = (next: number) => Math.min(max, Math.max(min, next));

  return (
    <div className="focus-ring-within flex h-9.5 w-24 items-stretch overflow-hidden rounded-lg border border-line bg-surface">
      <input
        id={id}
        type="number"
        aria-label={label}
        value={value}
        min={min}
        max={max}
        onChange={event => {
          const parsed = Number.parseInt(event.target.value, 10);
          if (!Number.isNaN(parsed)) onChange(clamp(parsed));
        }}
        className="w-full min-w-0 bg-transparent px-3 text-sm text-ink outline-none"
      />
      <div className="flex w-6 shrink-0 flex-col border-l border-line">
        <Stepper
          label={`Increase ${label.toLowerCase()}`}
          disabled={value >= max}
          onClick={() => onChange(clamp(value + 1))}
        >
          <ChevronUp size={12} />
        </Stepper>
        <Stepper
          label={`Decrease ${label.toLowerCase()}`}
          disabled={value <= min}
          onClick={() => onChange(clamp(value - 1))}
        >
          <ChevronDown size={12} />
        </Stepper>
      </div>
    </div>
  );
}

function Stepper({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="flex flex-1 cursor-pointer items-center justify-center bg-surface-muted text-ink-muted transition first:border-b first:border-line hover:text-ink disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  );
}

export function Banner({
  tone = "info",
  title,
  children,
  actions,
}: {
  tone?: "info" | "error";
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={cn(
        "flex items-start gap-3 rounded-xl border px-4 py-3",
        tone === "error" ? "border-danger/30 bg-danger-soft text-danger" : "border-brand/25 bg-brand-soft text-ink",
      )}
    >
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold">{title}</p>
        {children && <div className="mt-1 text-xs leading-relaxed">{children}</div>}
      </div>
      {actions && <div className="flex shrink-0 gap-2">{actions}</div>}
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 px-6 py-16 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-surface-muted text-ink-muted">
        {icon}
      </div>
      <div>
        <p className="text-sm font-semibold text-ink">{title}</p>
        <p className="mx-auto mt-1 max-w-sm text-xs text-ink-muted">{description}</p>
      </div>
      {action}
    </div>
  );
}

export function Divider() {
  return <div className="h-px bg-line" />;
}
