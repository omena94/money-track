export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}
/** 家庭檢視：正在查看哪位成員的帳本（null 為自己）。只套用在讀取請求。 */
let viewOwner: string | null = null;
export function setViewOwner(id: string | null) {
  viewOwner = id;
}
const ownScoped = /^\/(auth|health|household)(\/|$|\?)/;
export async function api<T>(
  path: string,
  options?: { method?: string; body?: unknown; csrf?: string },
): Promise<T> {
  const method = options?.method || "GET";
  const view = method === "GET" && viewOwner && !ownScoped.test(path);
  const res = await fetch(`/api/v1${path}`, {
    method,
    credentials: "same-origin",
    cache: "no-store",
    headers: {
      ...(options?.body ? { "Content-Type": "application/json" } : {}),
      ...(options?.csrf ? { "X-CSRF-Token": options.csrf } : {}),
      ...(view ? { "X-View-Owner": viewOwner! } : {}),
    },
    ...(options?.body ? { body: JSON.stringify(options.body) } : {}),
  });
  if (!res.ok) {
    const error = await res.json().catch(() => ({}));
    throw new ApiError(res.status, error.error || "連線中斷，請稍後再試");
  }
  return res.status === 204 ? (undefined as T) : res.json();
}
/** 頭像設定：與後端 users.avatar 相同 */
export type AvatarValue =
  | { type: "initial"; color?: string }
  | { type: "emoji"; emoji: string; color?: string }
  | { type: "photo"; v: string };
export type User = {
  id: string;
  name: string;
  email: string;
  is_admin: boolean;
  sync_approved: boolean;
  csrf_token: string;
  avatar?: AvatarValue | null;
};
export type Health = {
  status: string;
  database: boolean;
  redis: boolean;
  google_configured: boolean;
  login_url: string;
};
export type Account =
  import("./generated-api").components["schemas"]["Account"];
export type Entry = import("./generated-api").components["schemas"]["Entry"] & {
  read_only?: boolean;
  status?: string;
  source_details?: Record<string, string | null>;
  /** manual：手動紀錄；user：同步交易由使用者分類；rule：套用使用者規則；auto：內建關鍵字 */
  category_source?: "manual" | "user" | "rule" | "auto" | null;
};
export type CategoryRule = {
  id: string;
  pattern: string;
  category: string;
  created_at: string;
  matches: number;
};
export type DefaultCategoryRule = {
  pattern: string;
  category: string;
  kind: "income" | "expense" | null;
  priority: number;
};
export type SharedAccount = Pick<
  Account,
  "id" | "name" | "kind" | "currency" | "balance" | "institution"
>;
export type Dashboard = {
  totals: {
    currency: string;
    assets: string | null;
    liabilities: string | null;
    net: string | null;
    unknown_accounts: number;
  }[];
  monthly: { currency: string; income: string; expense: string }[];
  categories: { category: string; currency: string; amount: string }[];
};
export type Share = {
  id: string;
  account_id: string;
  recipient_email: string;
  permission: string;
  accepted: boolean;
  direction: string;
};
export type Connector = {
  id: string;
  name: string;
  mode: "api";
  manual_only: boolean;
  capabilities: string[];
  implementation: string;
  parser_verified: boolean;
  live_verified: boolean;
  credential_fields: string[];
};
export type Connection = {
  id: string;
  connector: string;
  configured: boolean;
  enabled: boolean;
  daily: boolean;
  manual_only?: boolean;
  /** 連續登入未成功次數；登入成功歸零 */
  login_failures?: number;
  last_success_at: string | null;
  record_count: number;
  next_run_at: string | null;
  counts?: Record<string, number>;
};
/** 所有來源共用的同步時間與登入保護 */
export type SyncSettings = {
  times: string[];
  attempt_limit: number;
  failure_limit: number;
  next_run_at: string;
  max_attempts: number;
  max_failures: number;
};
export type SourceRecord = {
  id: string;
  kind: string;
  data: Record<string, string | number | boolean | null>;
  current: boolean;
  observed_at: string;
  account_name: string | null;
  invoice_number?: string | null;
};
export type Job = {
  id: string;
  status: string;
  message: string;
  created_at: string;
  connector?: string;
  answer_pending?: boolean;
  summary?: Record<string, number>;
  challenge?: {
    id: string;
    kind: string;
    message: string;
    image?: string;
    channel?: string;
    choices?: { value: string; label: string }[];
    expires_at?: string;
  };
  progress?: {
    code: string;
    label: string;
    at: string;
    current?: number | null;
    total?: number | null;
  }[];
  /** 失敗原因代碼與失敗時所在的步驟代碼 */
  error_code?: string | null;
  failed_step?: string | null;
  attempts?: number;
};
/** 同步事件：文字由後端依白名單產生，detail 已遮罩 */
export type SyncEvent = {
  id: number;
  level: "info" | "warn" | "error";
  step: string | null;
  code: string;
  message: string;
  detail: string | null;
  at: string;
};
export type Member = {
  id: string;
  name: string;
  email: string;
  sync_approved: boolean;
  is_admin: boolean;
  avatar?: AvatarValue | null;
  created_at?: string;
  /** 最近一次登入（建立工作階段）時間 */
  last_login?: string | null;
  /** 最近通過身分驗證的活動時間；最密每 30 秒記錄 */
  last_active_at?: string | null;
  active_sessions?: number;
  /** 整合測試建立的帳號（@example.invalid） */
  test?: boolean;
};
/** GET /analysis：近 12 個月收支、分類×月份與當月明細（只計 TWD，不含內部資金移動） */
export type Analysis = {
  month: string;
  today: string;
  large_threshold: number;
  months: {
    month: string;
    income: string;
    expense: string;
    large: string;
    count: number;
  }[];
  matrix: { month: string; category: string; amount: string; count: number }[];
  entries: {
    id: string;
    date: string;
    kind: string;
    amount: string;
    description: string;
    category: string;
    account_name: string;
  }[];
  budgets: { category: string; amount: string }[];
  invoices: {
    count: number;
    amount: string;
    sellers: { name: string; amount: string; count: number }[];
    items: { name: string; amount: string; quantity: string; count: number }[];
  };
};
export type Budget = {
  id: string;
  month: string;
  category: string;
  amount: string;
  currency: string;
  spent: string;
};
export type Goal = {
  id: string;
  name: string;
  target: string;
  saved: string;
  currency: string;
  due_on: string | null;
};
export type Recurring = {
  id: string;
  account_name: string;
  currency: string;
  amount: string;
  description: string;
  frequency: string;
  next_on: string;
  enabled: boolean;
};
/** 訂閱：只記預期扣款，實際金額來自收支交易 */
export type Subscription = {
  id: string;
  name: string;
  category: string;
  amount: string;
  currency: string;
  cycle: "monthly" | "yearly";
  anchor_day: number;
  anchor_month: number | null;
  started_on: string;
  ended_on: string | null;
  paused: boolean;
  patterns: string[];
  tolerance: string;
  window_days: number;
  require_amount: boolean;
  note: string;
  /** 自訂圖示（data URL）；沒有上傳時用分類圖示 */
  icon: string | null;
};
export type SubscriptionSummary = Subscription & {
  estimate_twd: string | null;
  last_charge: { date: string; amount: string; currency: string | null } | null;
  review: number;
  history: { month: string; status: SubscriptionStatus }[];
};
export type SubscriptionStatus =
  "paid" | "upcoming" | "missed" | "review" | "inactive" | "none";
export type SubscriptionItem = {
  id: string;
  date: string;
  description: string | null;
  account_name: string | null;
  amount: string;
  currency: string | null;
  original_amount: string | null;
  original_currency: string | null;
  status: string;
  manual: boolean;
  role: "charge" | "fee" | "refund";
  match: "auto" | "confirmed" | "pending";
  amount_ok: boolean | null;
};
export type SubscriptionPeriod = {
  subscription: Subscription;
  month: string;
  due: string | null;
  status: SubscriptionStatus;
  flags: string[];
  estimate_twd: string | null;
  spent: { currency: string; amount: string }[];
  items: SubscriptionItem[];
  /** 這一期按過「不是」而排除的交易 */
  excluded?: {
    id: string;
    date: string;
    description: string | null;
    account_name: string | null;
    amount: string;
    currency: string | null;
  }[];
};
export type SubscriptionMonth = {
  month: string;
  today: string;
  summary: {
    actual: { currency: string; amount: string }[];
    expected_twd: string;
    monthly_equivalent_twd: string;
    unknown_estimate: number;
    paid: number;
    upcoming: number;
    missed: number;
    review: number;
  };
  periods: SubscriptionPeriod[];
};
export type SubscriptionSuggestion = {
  key: string;
  pattern: string;
  name: string | null;
  category: string;
  months: number;
  count: number;
  last_date: string;
  anchor_day: number;
  amount: string;
  currency: string | null;
  original_amount: string | null;
  original_currency: string | null;
  require_amount: boolean;
  recent: { date: string; amount: string }[];
};
export type Notification = {
  id: string;
  title: string;
  body: string;
  created_at: string;
};
export type Position = {
  id: string;
  account_id: string;
  account_name: string;
  currency: string;
  market: string;
  symbol: string;
  name: string;
  quantity: string;
  cost_basis: string | null;
  average_cost: string | null;
  last_price: string | null;
  market_value: string | null;
  unrealized_gain: string | null;
  price_date: string | null;
  price_source: string | null;
};
export type Trade = {
  id: string;
  symbol: string;
  name: string;
  kind: string;
  quantity: string;
  unit_price: string | null;
  cash_amount: string;
  fee: string;
  tax: string;
  realized_gain: string | null;
  occurred_on: string;
  currency: string;
  can_undo: boolean;
};
export type Quote = {
  market: string;
  symbol: string;
  name: string;
  price: string;
  quoted_on: string;
};
export type Quotes = {
  quotes: Quote[];
  sources: {
    source: string;
    rows: number;
    last_success: string | null;
    message: string;
  }[];
};
export type AssetCategory =
  "bank" | "time_deposit" | "investment" | "other" | "debt";
export type NetWorthPoint = {
  date: string;
  bank: number;
  time_deposit: number;
  investment: number;
  other: number;
  debt: number;
};
export type NetWorth = {
  as_of: string;
  currency: string;
  net: string;
  assets: string;
  liabilities: string;
  categories: { key: AssetCategory; amount: string; count: number }[];
  unknown: number;
  /** 外幣原幣淨額；twd／rate 為依匯率換算，不計入 net 與走勢 */
  foreign: {
    currency: string;
    net: string;
    twd?: string | null;
    rate?: string | null;
  }[];
  fx?:
    | (FxQuotes & {
        foreign_twd: string;
        complete: boolean;
        net_with_foreign: string;
      })
    | null;
  history: NetWorthPoint[];
};
export type FxQuotes = {
  source: string;
  quoted_at: string;
  fetched_at: string;
  /** buy：銀行買入（外幣換台幣），估值使用 */
  rates: { currency: string; buy: string; sell: string }[];
};
export type SourceAsset = {
  id: string;
  connection_id: string;
  connector: string;
  kind: "account" | "holding";
  category: AssetCategory;
  name: string;
  institution: string | null;
  account_type: string | null;
  mask: string | null;
  balance_from?: "bill" | null;
  reference_only?: boolean;
  linked_account?: {
    id: string;
    kind: "source" | "manual";
    name: string;
    mask: string | null;
    connector: string | null;
    available: boolean;
    included: boolean;
  } | null;
  symbol: string | null;
  quantity: string | null;
  asset_type?: string | null;
  cost_basis?: string | null;
  unit_price?: string | null;
  price_date?: string | null;
  unrealized_gain?: string | null;
  accumulated_dividend?: string | null;
  recurring_amount?: string | null;
  recurring_days?: string | null;
  source_status?: string | null;
  currency: string;
  amount: string | null;
  available_balance: string | null;
  credit_limit: string | null;
  payment_due: string | null;
  interest_rate: string | null;
  next_payment_amount?: string | null;
  last_payment_amount?: string | null;
  last_payment_on?: string | null;
  remaining_installments?: string | null;
  term: string | null;
  opened_on: string | null;
  maturity_on: string | null;
  as_of: string | null;
  observed_at: string;
  included: boolean;
};

export type HouseholdPerson = {
  id: string;
  email: string;
  name: string;
  avatar?: AvatarValue | null;
};
export type Household = {
  me: HouseholdPerson;
  viewers: { id: string; email: string; created_at: string }[];
  shared_with_me: (HouseholdPerson & { grant_id: string })[];
};
export type HouseholdSummary = {
  month: string;
  currency: string;
  net: string;
  assets: string;
  liabilities: string;
  members: (HouseholdPerson & {
    me: boolean;
    net: string;
    income: string;
    expense: string;
    /** 最近一次成功同步；沒有同步來源為 null */
    last_synced_at: string | null;
    connections: number;
    /** 啟用中但超過 48 小時未成功同步的來源數 */
    stale: number;
    failing: number;
  })[];
  history: NetWorthPoint[];
  months: { month: string; income: string; expense: string }[];
  categories: { category: string; amount: string }[];
  generated_at: string;
  /** 與上月同一段日期比較（本月未結束時只比到今天） */
  compare: {
    through_day: number;
    days_in_month: number;
    prev_month: string;
    prev_through_day: number;
    income: string;
    expense: string;
    prev_income: string;
    prev_expense: string;
    categories: { category: string; amount: string; prev: string }[];
  };
  /** 選取成員本月 TWD 預算加總；沒有人設預算為 null */
  budget: {
    amount: string;
    spent: string;
    categories: { category: string; amount: string; spent: string }[];
  } | null;
  internal_transfers: {
    count: number;
    amount: string;
    items: {
      date: string;
      amount: string;
      from: string;
      to: string;
      from_description: string;
      to_description: string;
    }[];
  };
  duplicates: { count: number; amount: string };
  shared_accounts: { name: string; members: string[] }[];
  foreign_count: number;
};
/** 家庭支出追查：兩段期間的合併支出明細（已去重、排除家庭內互轉，只計 TWD） */
export type HouseholdExplore = {
  currency: string;
  generated_at: string;
  today: string;
  members: { id: string; name: string; me: boolean }[];
  a: { members: string[]; period: { from: string; to: string; days: number } };
  b: { members: string[]; period: { from: string; to: string; days: number } };
  /** 支出為正、退款為負 */
  entries: {
    id: string;
    member_id: string;
    date: string;
    category: string;
    description: string;
    account: string | null;
    kind: string;
    amount: string;
  }[];
  truncated: boolean;
  /** A 期間結束月往前 12 個月，A 邊成員的分類月支出 */
  trend: {
    month: string;
    category: string;
    member_id: string;
    amount: string;
  }[];
  excluded: { transfers: number; duplicates: number };
};
export type UpcomingPayment = {
  id: string;
  source: "recurring" | "bill";
  date: string;
  title: string;
  category: string;
  account_name?: string;
  period?: string | null;
  amount: string;
  currency: string;
  /** scheduled：固定支出預計自動記帳；paid／unpaid／overdue 只依來源帳單資料 */
  status: "scheduled" | "paid" | "unpaid" | "overdue";
  last_synced_at?: string | null;
  member_id: string;
  member: string;
};
export type HouseholdUpcoming = {
  today: string;
  days: number;
  currency: string;
  week_due: string;
  period_due: string;
  overdue_count: number;
  items: UpcomingPayment[];
};
