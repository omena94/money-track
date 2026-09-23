import Decimal from "decimal.js";

const TZ = "Asia/Taipei";
const isoDate = (d: Date) =>
  new Intl.DateTimeFormat("en-CA", {
    timeZone: TZ,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(d);
export const today = () => isoDate(new Date());
export const daysAgo = (n: number) =>
  isoDate(new Date(Date.now() - n * 86400000));
export const thisMonth = () => today().slice(0, 7);
export function shiftMonth(month: string, delta: number) {
  const [y, m] = month.split("-").map(Number);
  const d = new Date(Date.UTC(y, m - 1 + delta, 1));
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
}
export function monthLabel(month: string) {
  const [y, m] = month.split("-").map(Number);
  return `${y} 年 ${m} 月`;
}
export function daysLeftInMonth() {
  const [y, m, d] = today().split("-").map(Number);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return last - d + 1;
}
const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
/** 2026-09-24 → 今天 · 9月24日 週四 */
export function dayLabel(date: string) {
  const [y, m, d] = date.split("-").map(Number);
  const w = weekdays[new Date(Date.UTC(y, m - 1, d)).getUTCDay()];
  const base = `${m}月${d}日 週${w}`;
  const thisYear = Number(today().slice(0, 4));
  const full = y === thisYear ? base : `${y}年${base}`;
  if (date === today()) return `今天 · ${full}`;
  if (date === daysAgo(1)) return `昨天 · ${full}`;
  return full;
}
export function shortDate(date: string) {
  const [, m, d] = date.split("-").map(Number);
  return `${m}/${d}`;
}
export function stamp(v: string | null | undefined) {
  return v
    ? new Date(v).toLocaleString("zh-TW", {
        timeZone: TZ,
        month: "numeric",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "尚未完成";
}
/** 時間戳 → 2026/09/25（台北時區，只取日期） */
export const stampDate = (v: string | null | undefined) =>
  v ? isoDate(new Date(v)).replaceAll("-", "/") : "—";
export function relative(v: string | null | undefined, now = Date.now()) {
  if (!v) return "從未";
  const diff = (now - new Date(v).getTime()) / 1000;
  if (diff < 60) return "剛剛";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分鐘前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小時前`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)} 天前`;
  return stamp(v);
}
export function greeting() {
  const h = Number(
    new Intl.DateTimeFormat("en-US", {
      timeZone: TZ,
      hour: "numeric",
      hour12: false,
    }).format(new Date()),
  );
  if (h < 5) return "夜深了";
  if (h < 11) return "早安";
  if (h < 14) return "午安";
  if (h < 18) return "下午好";
  return "晚安";
}

export const symbol = (currency: string) =>
  currency === "TWD" ? "NT$" : currency;
function grouped(n: Decimal, currency: string) {
  const digits = currency === "TWD" && n.isInteger() ? 0 : 2;
  return n
    .abs()
    .toFixed(digits)
    .replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}
/** NT$ 1,234 ／ -NT$ 1,234；null 表示資料待補 */
export const money = (value: string | null | undefined, currency = "TWD") => {
  if (value == null) return "資料待補";
  const n = new Decimal(value);
  return `${n.isNegative() && !n.isZero() ? "−" : ""}${symbol(currency)} ${grouped(n, currency)}`;
};
/** 帶正負號，交易列表用：+NT$ 100 ／ −NT$ 100 */
export const signed = (value: string, currency = "TWD") => {
  const n = new Decimal(value);
  const sign = n.isZero() ? "" : n.isNegative() ? "−" : "+";
  return `${sign}${symbol(currency)} ${grouped(n, currency)}`;
};
export const quantity = (v: string) =>
  Number(v).toLocaleString("zh-TW", { maximumFractionDigits: 8 });
export const percent = (v: number) =>
  `${v.toLocaleString("zh-TW", { maximumFractionDigits: 1 })}%`;

/** 只允許數字與 + - 的簡單算式，回傳 Decimal 字串；無效回傳 null */
export function evaluate(input: string): string | null {
  const s = input
    .replace(/[,\s，]/g, "")
    .replace(/[＋]/g, "+")
    .replace(/[－]/g, "-");
  if (!s) return null;
  if (!/^[+-]?\d*\.?\d+([+-]\d*\.?\d+)*$/.test(s)) return null;
  try {
    const terms = s.match(/[+-]?\d*\.?\d+/g) ?? [];
    return terms.reduce((a, t) => a.plus(t), new Decimal(0)).toString();
  } catch {
    return null;
  }
}

export const kinds: Record<string, string> = {
  cash: "現金",
  bank: "銀行帳戶",
  credit: "信用卡",
  investment: "投資帳戶",
  loan: "貸款",
  property: "不動產",
  insurance: "保險",
  other: "其他",
};
export const liabilityKinds = new Set(["credit", "loan"]);
export const currencies = [
  "TWD",
  "USD",
  "JPY",
  "EUR",
  "HKD",
  "CNY",
  "GBP",
  "AUD",
  "CAD",
  "SGD",
];
export const expenseCategories = [
  "餐飲",
  "交通",
  "購物",
  "居家",
  "娛樂",
  "醫療",
  "教育",
  "保險",
  "稅費",
  "手續費",
  "電子支付",
  "提款",
  "信用卡繳款",
  "貸款",
  "投資",
  "轉帳",
  "未分類",
];
export const incomeCategories = ["薪資", "獎金", "投資收益", "其他收入"];
export const categories = [...expenseCategories, ...incomeCategories];
export const entryKinds: Record<string, string> = {
  expense: "支出",
  income: "收入",
  transfer: "轉帳",
  refund: "退款",
  investment: "投資",
};
export const frequencies: Record<string, string> = {
  daily: "每日",
  weekly: "每週",
  monthly: "每月",
  yearly: "每年",
};
export const tradeKinds: Record<string, string> = {
  buy: "買入",
  sell: "賣出",
  dividend: "現金股利",
  opening: "期初持倉",
  basis: "補入成本",
};
export const markets: Record<string, string> = {
  TWSE: "上市",
  TPEX: "上櫃",
  OTHER: "海外／其他",
};
export const sourceNames: Record<string, string> = {
  esun: "玉山銀行",
  cathaybk: "國泰世華",
  sinopac: "永豐銀行",
  taishin: "台新銀行",
  ctbc: "中國信託",
  skbank: "新光銀行",
  hncb: "華南銀行",
  landbank: "土地銀行",
  obank: "王道銀行",
  firstbank: "第一銀行",
  fubon: "台北富邦銀行",
  hsbc: "滙豐銀行",
  richart: "Richart",
  tdcc: "集保 e 存摺",
  einvoice: "電子發票",
};
export const jobStatus: Record<string, { label: string; tone: string }> = {
  queued: { label: "排隊中", tone: "neutral" },
  running: { label: "執行中", tone: "info" },
  waiting_user: { label: "等你驗證", tone: "warn" },
  succeeded: { label: "已完成", tone: "good" },
  partial: { label: "部分完成", tone: "warn" },
  failed: { label: "失敗", tone: "bad" },
  cancelled: { label: "已取消", tone: "neutral" },
};
export const activeJob = (status: string) =>
  ["queued", "running", "waiting_user"].includes(status);
