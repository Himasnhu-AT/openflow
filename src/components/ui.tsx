// Theme.swift building blocks.
import { Fragment, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ChevronsUpDown, LoaderCircle, Check, type LucideIcon } from "lucide-react";

export function PageHeader({ title, subtitle, trailing }: { title: string; subtitle?: string; trailing?: ReactNode }) {
  return (
    <div className="page-header">
      <div style={{ minWidth: 0 }}>
        <div className="page-title">{title}</div>
        {subtitle && <div className="page-subtitle">{subtitle}</div>}
      </div>
      <div className="spacer" />
      {trailing}
    </div>
  );
}

export function Tabs<T extends string>({ items, value, onChange }: { items: [T, string][]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="tabs">
      <div className="tabs-strip">
        {items.map(([key, label]) => (
          <button key={key} className={"tab" + (key === value ? " on" : "")} onClick={() => onChange(key)}>
            <span>{label}</span>
            <div className="tab-underline" />
          </button>
        ))}
      </div>
      <div className="tabs-line" />
    </div>
  );
}

export function PrimaryButton({ title, onClick, style }: { title: string; onClick: () => void; style?: CSSProperties }) {
  return (
    <button className="btn-primary" onClick={onClick} style={style}>
      {title}
    </button>
  );
}

export function SecondaryButton({ title, onClick, destructive }: { title: string; onClick: () => void; destructive?: boolean }) {
  return (
    <button className={"btn-secondary" + (destructive ? " destructive" : "")} onClick={onClick}>
      {title}
    </button>
  );
}

export function IconButton({ icon: Icon, help, onClick }: { icon: LucideIcon; help?: string; onClick: () => void }) {
  return (
    <button className="icon-btn" title={help} aria-label={help} onClick={onClick}>
      <Icon size={14} strokeWidth={2} />
    </button>
  );
}

export function RowCard({ children, actions, style }: { children: ReactNode; actions?: ReactNode; style?: CSSProperties }) {
  return (
    <div className="row-card" style={style}>
      {children}
      {actions && <div className="row-actions">{actions}</div>}
    </div>
  );
}

/** CardList: rows separated by 1pt border lines. */
export function CardList({ children }: { children: ReactNode }) {
  const rows = (Array.isArray(children) ? children.flat() : [children]).filter(Boolean);
  return (
    <div className="card-list">
      {rows.map((row, i) => (
        <Fragment key={(row as { key?: string }).key ?? i}>
          {i > 0 && <div className="divider" />}
          {row}
        </Fragment>
      ))}
    </div>
  );
}

export function StatTile({ value, label, extra }: { value: string; label: string; extra?: ReactNode }) {
  return (
    <div className="stat-tile">
      <div className="stat-value">{value}</div>
      <div className="stat-label">{label}</div>
      {extra}
    </div>
  );
}

export const SectionLabel = ({ text }: { text: string }) => <div className="section-label">{text}</div>;

export function Dot({ color, size = 7 }: { color: string; size?: number }) {
  return <span className="dot" style={{ width: size, height: size, background: color }} />;
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <LoaderCircle className="spinner" size={size} />;
}

export function LiveBadge() {
  return (
    <span className="live-badge">
      <Dot color="var(--red)" size={6} />
      LIVE
    </span>
  );
}

export function Switch({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return <button role="switch" aria-checked={on} className={"switch" + (on ? " on" : "")} onClick={() => onChange(!on)} />;
}

export function Checkbox({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button role="checkbox" aria-checked={on} className={"checkbox" + (on ? " on" : "")} onClick={() => onChange(!on)}>
      {on && <Check size={11} strokeWidth={3.5} />}
    </button>
  );
}

export function Picker({
  value,
  options,
  onChange,
  width,
}: {
  value: string;
  options: [string, string][];
  onChange: (v: string) => void;
  width: number;
}) {
  const known = options.some(([v]) => v === value);
  return (
    <div className="picker" style={{ width }}>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {!known && <option value={value}>{value}</option>}
        {options.map(([v, label]) => (
          <option key={v} value={v}>
            {label}
          </option>
        ))}
      </select>
      <span className="picker-chevron">
        <ChevronsUpDown size={11} strokeWidth={2.5} />
      </span>
    </div>
  );
}

/** Segmented control (NSSegmentedControl): one of a few options, all visible. */
export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map(([v, label]) => (
        <button key={v} role="radio" aria-checked={v === value} className={"segment" + (v === value ? " on" : "")} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

/** Text input with local state that forwards every change (callers debounce the save). */
export function TextField({
  value,
  onChange,
  onSubmit,
  placeholder,
  width,
  type = "text",
  autoFocus,
  className = "field",
  style,
}: {
  value: string;
  onChange: (v: string) => void;
  onSubmit?: () => void;
  placeholder?: string;
  width?: number;
  type?: string;
  autoFocus?: boolean;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <input
      className={className}
      type={type}
      value={value}
      placeholder={placeholder}
      autoFocus={autoFocus}
      spellCheck={false}
      style={{ width, ...style }}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.nativeEvent.isComposing && onSubmit) onSubmit();
      }}
    />
  );
}

/** A textarea that grows with its content (min height given), like SwiftUI's TextEditor in a VStack. */
export function AutoTextArea({
  value,
  onChange,
  minHeight,
  className,
  style,
  autoFocus,
}: {
  value: string;
  onChange: (v: string) => void;
  minHeight: number;
  className?: string;
  style?: CSSProperties;
  autoFocus?: boolean;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = Math.max(minHeight, el.scrollHeight + 2) + "px";
  }, [value, minHeight]);
  return (
    <textarea
      ref={ref}
      className={className}
      style={style}
      value={value}
      autoFocus={autoFocus}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

/** Copy button whose icon flips to a checkmark for 1.5 s. */
export function useFlash(ms = 1500): [boolean, () => void] {
  const [on, setOn] = useState(false);
  const t = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(t.current), []);
  return [
    on,
    () => {
      setOn(true);
      window.clearTimeout(t.current);
      t.current = window.setTimeout(() => setOn(false), ms);
    },
  ];
}
