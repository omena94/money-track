"use client";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  Briefcase,
  Bus,
  CircleDashed,
  Clapperboard,
  Gift,
  GraduationCap,
  HeartPulse,
  House,
  Inbox,
  Loader2,
  ShoppingBag,
  Tag,
  TrendingUp,
  Utensils,
  X,
  ArrowLeftRight,
  Undo2,
  Coins,
  Smartphone,
  Banknote,
  CreditCard,
  Receipt,
  Shield,
  Landmark,
  HandCoins,
  ChevronLeft,
  ChevronRight,
  type LucideIcon,
} from "lucide-react";
import Decimal from "decimal.js";
import { money, shiftMonth, signed, thisMonth } from "@/lib/format";
import {
  colorVar,
  findCategory,
  iconOf,
  useCategories,
} from "@/lib/categories";
import { useApp } from "./app-context";

/* ---------- Money ---------- */
export function Money({
  value,
  currency = "TWD",
  sign = false,
  tone = false,
  className = "",
}: {
  value: string | null | undefined;
  currency?: string;
  /** 顯示 + / − */
  sign?: boolean;
  /** 正數套用收入色 */
  tone?: boolean;
  className?: string;
}) {
  const { hidden } = useApp();
  if (hidden)
    return (
      <span className={`num masked ${className}`} aria-label="金額已隱藏">
        ••••••
      </span>
    );
  if (value == null)
    return <span className={`num unknown ${className}`}>資料待補</span>;
  const positive =
    new Decimal(value).isPositive() && !new Decimal(value).isZero();
  return (
    <span
      className={`num ${tone && positive ? "pos" : ""} ${className}`.trim()}
    >
      {sign ? signed(value, currency) : money(value, currency)}
    </span>
  );
}
/** 台股慣例：紅漲綠跌，並附箭頭，不只靠顏色辨識 */
export function Gain({
  value,
  currency = "TWD",
  base,
}: {
  value: string | null;
  currency?: string;
  base?: string | null;
}) {
  const { hidden } = useApp();
  if (value == null) return <span className="num unknown">—</span>;
  const n = new Decimal(value);
  const cls = n.isZero() ? "" : n.isPositive() ? "up" : "down";
  const pct =
    base && !new Decimal(base).isZero()
      ? n.div(new Decimal(base).abs()).times(100).toFixed(2)
      : null;
  return (
    <span className={`num gain ${cls}`}>
      {n.isZero() ? "" : n.isPositive() ? "▲ " : "▼ "}
      {hidden ? "••••" : money(n.abs().toString(), currency)}
      {pct && <small> {Math.abs(Number(pct))}%</small>}
    </span>
  );
}

/* ---------- Sheet (modal / drawer / mobile bottom sheet) ---------- */
const stack: symbol[] = [];
export function Sheet({
  title,
  description,
  onClose,
  children,
  footer,
  size = "md",
  side = "center",
  busy = false,
}: {
  title: ReactNode;
  description?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
  side?: "center" | "right";
  busy?: boolean;
}) {
  const ref = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  close.current = () => {
    if (!busy) onClose();
  };
  const titleId = useId();
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  useEffect(() => {
    if (!mounted) return;
    const me = Symbol();
    stack.push(me);
    const previous = document.activeElement as HTMLElement | null;
    const el = ref.current;
    const selectors =
      'button:not([disabled]),input:not([disabled]):not([type="hidden"]),select:not([disabled]),textarea:not([disabled]),a[href],[tabindex="0"]';
    const first =
      el?.querySelector<HTMLElement>("[autofocus],[data-autofocus]") ??
      el?.querySelector<HTMLElement>(
        ".sheet-body " + selectors.split(",").join(",.sheet-body "),
      );
    (first ?? el)?.focus({ preventScroll: true });
    function key(e: KeyboardEvent) {
      if (stack[stack.length - 1] !== me) return;
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      }
      if (e.key === "Tab") {
        const items = Array.from(
          el?.querySelectorAll<HTMLElement>(selectors) ?? [],
        ).filter((i) => i.offsetParent !== null);
        if (!items.length) return;
        const a = items[0],
          z = items[items.length - 1];
        if (e.shiftKey && document.activeElement === a) {
          e.preventDefault();
          z.focus();
        } else if (!e.shiftKey && document.activeElement === z) {
          e.preventDefault();
          a.focus();
        }
      }
    }
    document.addEventListener("keydown", key);
    document.documentElement.classList.add("locked");
    return () => {
      document.removeEventListener("keydown", key);
      stack.splice(stack.indexOf(me), 1);
      if (!stack.length) document.documentElement.classList.remove("locked");
      previous?.focus?.({ preventScroll: true });
    };
  }, [mounted]);
  if (!mounted) return null;
  return createPortal(
    <div
      className={`scrim scrim-${side}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close.current();
      }}
    >
      <section
        ref={ref}
        tabIndex={-1}
        className={`sheet sheet-${side} sheet-${size}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <header className="sheet-head">
          <span className="grabber" aria-hidden="true" />
          <div>
            <h2 id={titleId}>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <button
            type="button"
            className="icon-btn"
            onClick={() => close.current()}
            aria-label="關閉"
          >
            <X size={20} />
          </button>
        </header>
        <div className="sheet-body">{children}</div>
        {footer && <footer className="sheet-foot">{footer}</footer>}
      </section>
    </div>,
    document.body,
  );
}

/* ---------- Buttons ---------- */
export function Button({
  variant = "secondary",
  size,
  busy,
  icon: Icon,
  children,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger" | "danger-ghost";
  size?: "sm" | "lg";
  busy?: boolean;
  icon?: LucideIcon;
}) {
  return (
    <button
      type="button"
      {...rest}
      disabled={rest.disabled || busy}
      className={`btn btn-${variant} ${size ? `btn-${size}` : ""} ${className}`.trim()}
    >
      {busy ? (
        <Loader2 className="spin" size={16} />
      ) : (
        Icon && <Icon size={size === "sm" ? 15 : 17} />
      )}
      {children}
    </button>
  );
}

/* ---------- Form ---------- */
export function Field({
  label,
  hint,
  children,
  optional,
  className = "",
}: {
  label: ReactNode;
  hint?: ReactNode;
  optional?: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <label className={`field ${className}`}>
      <span className="field-label">
        {label}
        {optional && <em>選填</em>}
      </span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  label,
  size,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode; disabled?: boolean }[];
  label: string;
  size?: "sm";
}) {
  return (
    <div
      className={`segmented ${size === "sm" ? "segmented-sm" : ""}`}
      role="radiogroup"
      aria-label={label}
    >
      {options.map((o) => (
        <button
          type="button"
          role="radio"
          aria-checked={value === o.value}
          key={o.value}
          disabled={o.disabled}
          className={value === o.value ? "on" : ""}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
export function Chip({
  active,
  children,
  onClick,
  icon: Icon,
  title,
}: {
  active?: boolean;
  children: ReactNode;
  onClick: () => void;
  icon?: LucideIcon;
  title?: string;
}) {
  return (
    <button
      type="button"
      className={`chip ${active ? "on" : ""}`}
      aria-pressed={!!active}
      onClick={onClick}
      title={title}
    >
      {Icon && <Icon size={15} />}
      {children}
    </button>
  );
}
export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`switch ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}
export function FormError({ children }: { children?: ReactNode }) {
  if (!children) return null;
  return (
    <div className="form-error" role="alert">
      {children}
    </div>
  );
}

/* ---------- Display ---------- */
export function Progress({
  value,
  tone,
  label,
}: {
  value: number;
  tone?: "good" | "warn" | "bad" | "accent";
  label?: string;
}) {
  const v = Math.max(0, Math.min(100, value));
  return (
    <div
      className={`progress ${tone ?? ""}`}
      role="progressbar"
      aria-valuenow={Math.round(v)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={label}
    >
      <span style={{ width: `${v}%` }} />
    </div>
  );
}
export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: string;
  children: ReactNode;
}) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}
export function Empty({
  icon: Icon = Inbox,
  title,
  children,
  action,
  compact,
}: {
  icon?: LucideIcon;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  compact?: boolean;
}) {
  return (
    <div className={`empty ${compact ? "compact" : ""}`}>
      <span className="empty-icon">
        <Icon size={compact ? 20 : 24} />
      </span>
      <strong>{title}</strong>
      {children && <p>{children}</p>}
      {action}
    </div>
  );
}
export function Skeleton({ rows = 3, h = 52 }: { rows?: number; h?: number }) {
  return (
    <div className="skeleton-list" aria-busy="true" aria-label="讀取中">
      {Array.from({ length: rows }, (_, i) => (
        <span key={i} className="skeleton" style={{ height: h }} />
      ))}
    </div>
  );
}
export function Card({
  title,
  action,
  children,
  className = "",
  pad = true,
}: {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  pad?: boolean;
}) {
  return (
    <section className={`card ${pad ? "" : "flush"} ${className}`}>
      {(title || action) && (
        <header className="card-head">
          {title && <h2>{title}</h2>}
          {action}
        </header>
      )}
      {children}
    </section>
  );
}
export function PageHead({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="page-head">
      <div>
        <h1>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </div>
  );
}
export function ErrorLine({
  children,
  retry,
}: {
  children: ReactNode;
  retry?: () => void;
}) {
  return (
    <div className="inline-error" role="alert">
      <span>{children}</span>
      {retry && (
        <button type="button" className="link" onClick={retry}>
          重試
        </button>
      )}
    </div>
  );
}

/* ---------- Category visuals ---------- */
const categoryIcons: Record<string, LucideIcon> = {
  餐飲: Utensils,
  交通: Bus,
  購物: ShoppingBag,
  居家: House,
  娛樂: Clapperboard,
  醫療: HeartPulse,
  教育: GraduationCap,
  薪資: Briefcase,
  獎金: Gift,
  投資收益: TrendingUp,
  其他收入: Coins,
  轉帳: ArrowLeftRight,
  電子支付: Smartphone,
  提款: Banknote,
  信用卡繳款: CreditCard,
  投資: TrendingUp,
  手續費: Receipt,
  保險: Shield,
  稅費: Landmark,
  貸款: HandCoins,
  未分類: CircleDashed,
};
export const palette = [
  "var(--c1)",
  "var(--c2)",
  "var(--c3)",
  "var(--c4)",
  "var(--c5)",
  "var(--c6)",
  "var(--c7)",
  "var(--c8)",
];
export function categoryColor(name: string) {
  if (name === "未分類") return "var(--ink-3)";
  const mine = findCategory(name);
  if (mine) return colorVar(mine.color);
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const fixed = Object.keys(categoryIcons).indexOf(name);
  return palette[(fixed >= 0 ? fixed : h) % palette.length];
}
export function CategoryIcon({
  category,
  kind,
  size = 40,
}: {
  category: string;
  kind?: string;
  size?: number;
}) {
  useCategories(); // 分類設定變更時重新繪製
  const mine = findCategory(category);
  const Icon =
    kind === "transfer"
      ? ArrowLeftRight
      : kind === "refund"
        ? Undo2
        : kind === "investment"
          ? TrendingUp
          : ((mine && iconOf(mine.icon)) ?? categoryIcons[category] ?? Tag);
  const color =
    kind === "transfer" || kind === "investment"
      ? "var(--ink-3)"
      : categoryColor(category);
  return (
    <span
      className="cat-icon"
      style={{ width: size, height: size, ["--cat" as string]: color }}
      aria-hidden="true"
    >
      <Icon size={Math.round(size * 0.45)} />
    </span>
  );
}
export { categoryIcons };

/** 月份切換：‹ 2026 年 9 月 › ，不在本月時多一個「本月」 */
export function MonthSwitch({
  month,
  onChange,
  max,
}: {
  month: string;
  onChange: (m: string) => void;
  /** 可切換到的最後一個月；不給則不限制 */
  max?: string;
}) {
  const current = thisMonth();
  return (
    <div className="month-switch" role="group" aria-label="切換月份">
      <button
        type="button"
        aria-label="上個月"
        onClick={() => onChange(shiftMonth(month, -1))}
      >
        <ChevronLeft size={18} />
      </button>
      <strong>
        {month.slice(0, 4)} 年 {Number(month.slice(5))} 月
      </strong>
      <button
        type="button"
        aria-label="下個月"
        disabled={max != null && month >= max}
        onClick={() => onChange(shiftMonth(month, 1))}
      >
        <ChevronRight size={18} />
      </button>
      {month !== current && (
        <button
          type="button"
          className="month-switch-now"
          onClick={() => onChange(current)}
        >
          本月
        </button>
      )}
    </div>
  );
}

/** 只在自己的帳本顯示；檢視家人帳本（唯讀）時隱藏所有寫入入口 */
export function Writable({ children }: { children: ReactNode }) {
  const { viewing } = useApp();
  return viewing ? null : <>{children}</>;
}
