"use client";
import {
  useEffect,
  useMemo,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleSlash,
  CircleX,
  Clock,
  Settings2,
  Database,
  KeyRound,
  Landmark,
  Loader2,
  Play,
  Plus,
  PlugZap,
  RefreshCw,
  ReceiptText,
  Search,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  TrendingUp,
  X,
  Zap,
  BookOpen,
} from "lucide-react";
import Decimal from "decimal.js";
import {
  api,
  type Connection,
  type Connector,
  type Health,
  type Job,
  type SourceRecord,
  type SyncSettings,
} from "@/lib/api";
import {
  activeJob,
  money,
  quantity,
  relative,
  signed,
  sourceNames,
  stamp,
} from "@/lib/format";
import { useJobsContext } from "@/lib/queries";
import { SyncSettingsSheet, useSyncSettings } from "./sync-settings";
import { SyncErrorCard, SyncLog } from "./sync-log";
import { GmailVerification } from "./gmail-verification";
import { BankLogo } from "@/components/bank-logo";
import { useAction, useApp, useDebounced } from "@/components/app-context";
import {
  Button,
  Card,
  Empty,
  ErrorLine,
  Field,
  FormError,
  PageHead,
  Sheet,
  Skeleton,
  Switch,
} from "@/components/ui";

const fields: Record<string, string> = {
  userId: "身分證字號",
  nationalId: "身分證字號",
  account: "使用者代號",
  alias: "使用者代號",
  password: "登入密碼",
  mobile: "手機號碼",
};
/** 各類資料的顯示名稱與單位，依重要性排序 */
const kindInfo: Record<string, { label: string; unit: string }> = {
  account: { label: "帳戶", unit: "個" },
  card: { label: "卡片", unit: "張" },
  holding: { label: "持股", unit: "檔" },
  bill: { label: "信用卡帳單", unit: "期" },
  investment_transaction: { label: "投資交易", unit: "筆" },
  transaction: { label: "帳戶收支", unit: "筆" },
  net_worth: { label: "資產歷史", unit: "筆" },
  invoice: { label: "電子發票", unit: "張" },
  invoice_item: { label: "發票品項", unit: "項" },
};
const kindOrder = Object.keys(kindInfo);
/** 來源的類型標籤 */
const sourceType = (id: string) =>
  id === "tdcc" ? "證券" : id === "einvoice" ? "發票" : "銀行";
/** 連結前讓使用者知道會拿到什麼、需要做什麼 */
const guide: Record<string, { offers: string[]; notes: string[] }> = {
  yuanta: {
    offers: [
      "臺幣存款帳面餘額與可用餘額",
      "近六個月逐筆交易、交易後餘額與備註",
      "持卡資訊、銀行可查的六期帳單與繳款期限",
    ],
    notes: [
      "直接透過 HTTP 同步，每次使用新的登入連線，完成後登出",
      "圖形驗證碼自動辨識與換圖；額外 OTP 或裝置驗證另行處理",
      "非空信用卡消費、定存、外幣與信託資料仍待驗證，遇到時標示部分完成並保留既有資料",
    ],
  },
  landbank: {
    offers: ["臺幣存款可用餘額", "近兩年逐筆交易、交易後餘額與備註"],
    notes: [
      "土地銀行使用獨立登入設定，與其他銀行分開",
      "本機 HTTP 與圖形驗證碼辨識",
      "餘額依銀行顯示的可用餘額計入資產",
      "非空外幣、定存及信用卡尚待驗證，遇到時標示部分完成",
      "逐段查詢並保留間隔；連續登入未成功達設定次數即暫停",
    ],
  },
  hncb: {
    offers: [
      "臺幣與外幣存款餘額、可用餘額",
      "銀行可查的近兩年逐筆交易、交易後餘額與備註",
    ],
    notes: [
      "華南使用獨立登入設定，與 Richart 分開",
      "本機 HTTP 與圖形驗證碼辨識",
      "非空定存及信用卡帳單尚待驗證，遇到時標示部分完成",
      "連續登入未成功達設定次數即暫停",
    ],
  },
  hsbc: {
    offers: [
      "台幣與外幣存款餘額、最近 180 天交易",
      "房貸餘額、利率、還款日與金額、剩餘期數",
    ],
    notes: [
      "僅支援手動同步，每次都要輸入手機 App 新產生的六位網銀認證碼",
      "登入與帳務查詢直接透過 HTTP",
      "按下同步後，在此頁輸入「登入網路銀行」認證碼，請勿輸入 App 固定 PIN",
      "可查詢存款與房貸；部分資料未取得時會標示部分完成，不含信用卡與信託",
    ],
  },
  richart: {
    offers: [
      "Richart 臺幣活存餘額與可用餘額",
      "最近 12 個月逐筆收支、交易後餘額與備註",
    ],
    notes: [
      "Richart 與台新是獨立來源，登入設定及帳戶不合併",
      "本機 HTTP 與圖形驗證碼辨識",
      "非空外幣、定存與子帳戶仍待驗證，遇到時顯示部分完成；本入口未提供信用卡帳單同步",
      "連續登入未成功達設定次數即暫停",
    ],
  },
  fubon: {
    offers: [
      "基金持倉、投入本金、市值、淨值與累積配息",
      "存款餘額、銀行提供的最近一年逐筆明細",
      "信用卡、可查的 6 期帳單與未出帳消費",
      "繳款期限、已繳金額與目前使用額度",
    ],
    notes: [
      "本機 HTTP 與圖形驗證碼辨識",
      "非空外幣／定存明細仍待驗證，遇到時顯示部分完成",
      "連續登入未成功達設定次數即暫停，額外裝置驗證會顯示待處理狀態",
    ],
  },
  firstbank: {
    offers: [
      "臺幣活存、逐筆交易與定存到期日",
      "銀行可查的一至兩年存款明細",
      "信用卡、12 期帳單、未出帳與繳款期限",
    ],
    notes: [
      "本機 HTTP 與圖形驗證碼辨識",
      "非空外幣產品仍待驗證，遇到時顯示部分完成",
      "連續登入未成功達設定次數即暫停，請確認銀行狀態後更新登入設定",
    ],
  },
  einvoice: {
    offers: [
      "全部歸戶載具的電子發票、店家與金額",
      "商品品名、數量、單價與折扣",
      "本期及前一期發票（約 3–4 個月）",
    ],
    notes: [
      "透過財政部網站同步，每天自動更新",
      "使用手機號碼與網站驗證碼（密碼）；需要時會請你完成圖形驗證",
      "發票獨立呈現，尚未與銀行消費配對，不重複計入收支",
    ],
  },
  tdcc: {
    offers: [
      "持股明細與每日市值（股票、ETF、基金）",
      "各銀行交割帳戶餘額，含活存與定存",
      "買賣、配息等投資交易紀錄",
      "過去一年的資產走勢",
    ],
    notes: [
      "直接連線，不需要開啟瀏覽器",
      "第一次登入可能需要輸入 Email 或簡訊驗證碼",
    ],
  },
  esun: {
    offers: [
      "信用卡額度與餘額",
      "最近 12 個月份的已出帳消費、繳款及回饋",
      "未出帳消費與帳單繳款期限",
    ],
    notes: [
      "本機 HTTP 同步",
      "目前已驗證信用卡；非空存款、外幣存款及貸款仍待映射，遇到時顯示部分完成",
    ],
  },
  sinopac: {
    offers: ["臺幣、外幣活存餘額", "銀行可查的最近一年逐筆存款交易"],
    notes: [
      "本機 HTTP 與圖形驗證碼辨識",
      "信用卡及定存仍待真實帳戶驗證；有這些產品時顯示部分完成",
      "連續登入未成功達設定次數即暫停，請確認銀行狀態後更新登入設定",
    ],
  },
  cathaybk: {
    offers: [
      "存款帳戶與餘額（臺幣、外幣）",
      "銀行可查期間內的存款交易（逐年完整查詢）",
      "信用卡額度與應繳金額",
      "全部可查信用卡帳單、逐筆消費與未出帳明細",
    ],
    notes: [
      "使用本地 HTTP 直接同步；銀行要求額外驗證時會停止並顯示狀態",
      "網銀登入可能讓你手機上的國泰世華 App 被登出",
      "未出帳消費依銀行回傳時間更新；信用卡繳款不重複列為支出",
    ],
  },
};
const SourceLogo = ({ id, size = 40 }: { id: string; size?: number }) => (
  <BankLogo
    names={[id, sourceNames[id]]}
    size={size}
    fallback={
      id === "tdcc" ? TrendingUp : id === "einvoice" ? ReceiptText : Landmark
    }
    tone={
      id === "einvoice" ? "var(--c7)" : id === "tdcc" ? "var(--c6)" : undefined
    }
  />
);

type Status = {
  tone: "good" | "info" | "warn" | "bad" | "neutral";
  icon: typeof CircleCheck;
  label: string;
};
function sourceStatus(c: Connection, jobs: Job[], limit = 2): Status {
  const active = jobs.find((j) => activeJob(j.status));
  if (!c.configured)
    return { tone: "neutral", icon: CircleSlash, label: "未連結" };
  if (c.enabled && (c.login_failures ?? 0) >= limit)
    return { tone: "bad", icon: ShieldAlert, label: "登入保護暫停" };
  if (active?.status === "waiting_user")
    return { tone: "bad", icon: ShieldAlert, label: "等你驗證" };
  if (active) return { tone: "info", icon: Loader2, label: "同步中" };
  if (!c.enabled)
    return { tone: "neutral", icon: CircleSlash, label: "已暫停" };
  const last = jobs.find(
    (j) => !activeJob(j.status) && j.status !== "cancelled",
  );
  if (last?.status === "failed")
    return { tone: "bad", icon: CircleX, label: "同步失敗" };
  if (last?.status === "partial")
    return { tone: "warn", icon: CircleAlert, label: "部分完成" };
  if (!c.last_success_at)
    return { tone: "warn", icon: Clock, label: "尚未同步" };
  return { tone: "good", icon: CircleCheck, label: "正常" };
}
const rank: Record<Status["tone"], number> = {
  bad: 0,
  warn: 1,
  info: 2,
  good: 3,
  neutral: 4,
};
function dataSummary(c: Connection) {
  const counts = c.counts ?? {};
  const parts = ["account", "holding", "bill", "invoice", "invoice_item"]
    .filter((k) => counts[k])
    .map((k) => `${counts[k]} ${kindInfo[k].unit}${kindInfo[k].label}`);
  return (
    parts.join(" · ") ||
    (c.record_count ? `${c.record_count} 筆資料` : "尚無資料")
  );
}

export function SourcesPage({ health }: { health?: Health }) {
  const { viewing } = useApp();
  if (viewing) return <ReadOnlyNotice what="資料來源" />;
  return <Sources health={health} />;
}
function Sources({ health }: { health?: Health }) {
  const { user, route, navigate } = useApp();
  const approved = user.is_admin || user.sync_approved;
  const { jobs } = useJobsContext();
  const catalog = useQuery({
    queryKey: ["connectors"],
    queryFn: () => api<Connector[]>("/connectors"),
  });
  const connections = useQuery({
    queryKey: ["connections"],
    queryFn: () => api<Connection[]>("/connections"),
    refetchInterval: 10000,
  });
  const { run, busy } = useAction();
  const sync = useSyncSettings();
  const limit = sync.data?.failure_limit ?? 2;
  // 詳細頁跟著網址走，瀏覽器上一頁可回到列表
  const detail = route.params.get("source") ?? undefined;
  const setDetail = (id?: string) =>
    navigate("sources", id ? { source: id } : undefined);
  const [adding, setAdding] = useState(false);
  const [scheduling, setScheduling] = useState(false);
  const waiting = jobs.filter((j) => j.status === "waiting_user");
  const byId = new Map((connections.data ?? []).map((c) => [c.connector, c]));
  const catalogList = (catalog.data ?? []).map((s) => ({
    ...s,
    manual_only: s.manual_only || byId.get(s.id)?.manual_only === true,
  }));
  // 已連結，或取消連結但仍保留資料的來源
  const rows = useMemo(
    () =>
      catalogList
        .flatMap((s) => {
          const c = byId.get(s.id);
          if (!c || (!c.configured && !c.record_count)) return [];
          const js = jobs.filter((j) => j.connector === s.id);
          return [{ s, c, jobs: js, status: sourceStatus(c, js, limit) }];
        })
        .sort(
          (a, b) =>
            rank[a.status.tone] - rank[b.status.tone] ||
            a.s.name.localeCompare(b.s.name, "zh-TW"),
        ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [catalog.data, connections.data, jobs, limit],
  );
  const addable = catalogList.filter((s) => !byId.get(s.id)?.configured);
  const syncable = rows.filter(
    (r) =>
      r.c.configured &&
      r.c.enabled &&
      !r.s.manual_only &&
      (r.c.login_failures ?? 0) < limit,
  );
  const problems = rows.filter(
    (r) => r.status.tone === "bad" || r.status.tone === "warn",
  );
  const lastSync = rows
    .map((r) => r.c.last_success_at)
    .filter(Boolean)
    .sort()
    .at(-1);
  const open = rows.find((r) => r.s.id === detail);
  const recent = jobs
    .filter((j) => j.connector !== "browser-diagnostic" && !activeJob(j.status))
    .slice(0, 5);

  const sheets = (
    <>
      {scheduling && (
        <SyncSettingsSheet
          settings={sync.data}
          onClose={() => setScheduling(false)}
        />
      )}
      {adding && (
        <AddSource
          sources={addable}
          onClose={() => setAdding(false)}
          onOpen={(id) => {
            setAdding(false);
            setDetail(id);
          }}
        />
      )}
    </>
  );

  if (open)
    return (
      <>
        <SourceDetail
          source={open.s}
          connection={open.c}
          jobs={open.jobs}
          status={open.status}
          approved={approved}
          sync={sync.data}
          waiting={waiting.filter((j) => j.connector === open.s.id)}
          onClose={() => setDetail(undefined)}
        />
        {sheets}
      </>
    );
  if (detail && (catalog.isPending || connections.isPending))
    return <Skeleton rows={4} h={96} />;

  return (
    <>
      <PageHead
        title="資料來源"
        subtitle="連結銀行、證券與電子發票，自動更新財務資料"
        actions={
          <Button
            variant="primary"
            icon={Plus}
            disabled={!approved || !addable.length}
            onClick={() => setAdding(true)}
          >
            新增資料來源
          </Button>
        }
      />
      <div className="stack">
        {!approved && (
          <div className="callout warn">
            <ShieldAlert size={18} />
            <span>同步功能需要管理員核准後才能使用，請聯絡管理員開通。</span>
          </div>
        )}

        {waiting.length > 0 && (
          <section className="card urgent" aria-label="需要你驗證">
            <header className="card-head">
              <h2>
                <ShieldAlert size={18} /> 需要你驗證
              </h2>
              <span className="muted small">請在 10 分鐘內完成</span>
            </header>
            {waiting.map((j) => (
              <Challenge key={j.id} job={j} />
            ))}
          </section>
        )}

        {catalog.isPending || connections.isPending ? (
          <Skeleton rows={3} h={64} />
        ) : catalog.isError || connections.isError ? (
          <ErrorLine
            retry={() => {
              void catalog.refetch();
              void connections.refetch();
            }}
          >
            資料來源讀取失敗
          </ErrorLine>
        ) : !rows.length ? (
          <section className="card sources-empty">
            <Empty
              icon={PlugZap}
              title="還沒有連結任何資料來源"
              action={
                <Button
                  variant="primary"
                  icon={Plus}
                  disabled={!approved}
                  onClick={() => setAdding(true)}
                >
                  新增資料來源
                </Button>
              }
            >
              連結集保或銀行後，餘額、持股與交易會帶入資產與淨資產，依各來源設定更新。
            </Empty>
          </section>
        ) : (
          <section className="card flush source-table">
            <header className="source-table-head">
              <div>
                <strong>已連結 {rows.length} 個來源</strong>
                <small className={problems.length ? "warn-text" : ""}>
                  {problems.length
                    ? `${problems.length} 個需要處理`
                    : "全部正常"}
                  {lastSync && ` · 最近同步 ${relative(lastSync)}`}
                </small>
              </div>
              <button
                type="button"
                className="sync-plan"
                onClick={() => setScheduling(true)}
                title="設定同步時間與登入保護"
              >
                <Clock size={15} />
                <span>
                  {sync.data
                    ? `每天 ${sync.data.times.join("、")}`
                    : "同步時間"}
                  {sync.data && (
                    <small>下次 {stamp(sync.data.next_run_at)}</small>
                  )}
                </span>
                <Settings2 size={15} />
              </button>
              <Button
                size="sm"
                icon={RefreshCw}
                busy={busy}
                disabled={!approved || !syncable.length}
                onClick={async () => {
                  for (const r of syncable)
                    await run(`/connections/${r.c.id}/sync`, {
                      toastError: true,
                    });
                }}
              >
                全部同步
              </Button>
            </header>
            <div className="st-row st-head" aria-hidden="true">
              <span>來源</span>
              <span>資料</span>
              <span>上次同步</span>
              <span>自動同步</span>
              <span />
            </div>
            <ul>
              {rows.map(({ s, c, jobs: js, status }) => (
                <SourceRow
                  key={s.id}
                  source={s}
                  connection={c}
                  status={status}
                  syncing={js.some((j) => activeJob(j.status))}
                  active={js.find((j) => activeJob(j.status))}
                  approved={approved}
                  limit={limit}
                  onOpen={() => setDetail(s.id)}
                />
              ))}
            </ul>
            {addable.length > 0 && approved && (
              <button
                type="button"
                className="st-add"
                onClick={() => setAdding(true)}
              >
                <Plus size={16} />
                新增資料來源
                <small>
                  還可以連結 {addable.map((s) => s.name).join("、")}
                </small>
              </button>
            )}
          </section>
        )}

        {recent.length > 0 && (
          <Card title="最近同步" pad={false}>
            <ol className="timeline">
              {recent.map((j) => (
                <TimelineItem key={j.id} job={j} />
              ))}
            </ol>
          </Card>
        )}

        <Advanced health={health} approved={approved} isAdmin={user.is_admin} />
      </div>

      {sheets}
    </>
  );
}

/* ---------------- 列表 ---------------- */
function SourceRow({
  source: s,
  connection: c,
  status,
  syncing,
  approved,
  onOpen,
  active,
  limit,
}: {
  source: Connector;
  connection: Connection;
  status: Status;
  syncing: boolean;
  approved: boolean;
  onOpen: () => void;
  active?: Job;
  limit: number;
}) {
  const paused = c.enabled && (c.login_failures ?? 0) >= limit;
  const { run, busy } = useAction();
  return (
    <li className="st-row connector-card connected">
      <button
        type="button"
        className="st-main"
        onClick={onOpen}
        aria-label={`${s.name}詳細資料`}
      >
        <SourceLogo id={s.id} />
        <span className="st-name">
          <strong>{s.name}</strong>
          <span>
            <span className={`dot ${status.tone}`} />
            {active ? active.message : status.label}
            <em>{sourceType(s.id)}</em>
          </span>
        </span>
      </button>
      <span className="st-cell" data-label="資料">
        {dataSummary(c)}
      </span>
      <span className="st-cell" data-label="上次同步">
        {c.last_success_at ? relative(c.last_success_at) : "—"}
      </span>
      <span className="st-cell" data-label="自動同步">
        {paused ? (
          <span className="bad-text">
            已暫停 · 連續失敗 {c.login_failures} 次
          </span>
        ) : c.configured && c.enabled && s.manual_only ? (
          "手動 · 每次驗證"
        ) : c.configured && c.enabled && c.daily ? (
          "自動"
        ) : c.configured && c.enabled ? (
          "手動"
        ) : (
          "—"
        )}
      </span>
      <span className="st-actions">
        {c.configured && (
          <button
            type="button"
            className="icon-btn"
            title={`同步${s.name}`}
            aria-label={`同步${s.name}`}
            disabled={!approved || !c.enabled || busy || syncing || paused}
            onClick={() =>
              run(`/connections/${c.id}/sync`, {
                success: `已開始同步${s.name}`,
                toastError: true,
              })
            }
          >
            {syncing ? (
              <Loader2 size={17} className="spin" />
            ) : (
              <Play size={17} />
            )}
          </button>
        )}
        <button
          type="button"
          className="icon-btn"
          aria-label={`開啟${s.name}`}
          onClick={onOpen}
        >
          <ChevronRight size={18} />
        </button>
      </span>
    </li>
  );
}

/* ---------------- 詳細面板 ---------------- */
function SourceDetail({
  source: s,
  connection: c,
  jobs,
  status,
  approved,
  sync,
  waiting,
  onClose,
}: {
  source: Connector;
  connection: Connection;
  jobs: Job[];
  status: Status;
  approved: boolean;
  sync?: SyncSettings;
  waiting: Job[];
  onClose: () => void;
}) {
  const limit = sync?.failure_limit ?? 2;
  const failures = c.login_failures ?? 0;
  const paused = c.configured && c.enabled && failures >= limit;
  const { run, busy } = useAction();
  const [viewing, setViewing] = useState<string | true>();
  const [editing, setEditing] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [liveLog, setLiveLog] = useState(true);
  const [failedLog, setFailedLog] = useState(false);
  const syncing = jobs.some((j) => activeJob(j.status));
  const activeNow = jobs.find((j) => activeJob(j.status));
  const lastFailed = jobs.find(
    (j) => !activeJob(j.status) && j.status !== "cancelled",
  );
  const counts = c.counts ?? {};
  const kinds = kindOrder.filter((k) => counts[k]);
  const history = jobs.filter((j) => !activeJob(j.status)).slice(0, 5);
  const startSync = () =>
    run(`/connections/${c.id}/sync`, {
      success: `已開始同步${s.name}`,
      toastError: true,
    });
  const patch = (enabled: boolean, daily: boolean, success: string) =>
    run(`/connections/${c.id}`, {
      method: "PATCH",
      body: { enabled, daily: s.manual_only ? false : daily },
      success,
      toastError: true,
    });
  const primary = c.configured ? (
    <Button
      variant="primary"
      icon={Play}
      busy={syncing}
      disabled={!approved || !c.enabled || busy}
      onClick={startSync}
    >
      {syncing ? "同步中…" : "立即同步"}
    </Button>
  ) : (
    <Button
      variant="primary"
      icon={KeyRound}
      disabled={!approved}
      onClick={() => setEditing(true)}
    >
      重新連結
    </Button>
  );
  return (
    <div className="source-page">
      <button type="button" className="back-link" onClick={onClose}>
        <ArrowLeft size={16} /> 資料來源
      </button>
      <header className="source-page-head">
        <SourceLogo id={s.id} size={52} />
        <div>
          <h1 className="detail-title">
            {s.name}
            <span className={`status-pill ${status.tone}`}>
              <status.icon size={13} className={syncing ? "spin" : ""} />
              {status.label}
            </span>
          </h1>
          <p>{sourceType(s.id)} · 直接連線</p>
        </div>
        <div className="page-actions">{primary}</div>
      </header>

      <div className="source-detail">
        {waiting.length > 0 && (
          <section className="card urgent" aria-label="需要你驗證">
            <header className="card-head">
              <h2>
                <ShieldAlert size={18} /> 需要你驗證
              </h2>
              <span className="muted small">請在 10 分鐘內完成</span>
            </header>
            {waiting.map((j) => (
              <Challenge key={j.id} job={j} />
            ))}
          </section>
        )}
        {s.manual_only && (
          <div className="callout">
            <KeyRound size={16} />
            <span>
              {s.id === "fubon"
                ? "此帳號目前採手動同步。可按「立即同步」完成 App／Email／簡訊驗證；也可連結 Gmail，在自動取碼驗收成功後依你的設定開啟排程。"
                : "僅支援手動同步，每次都要輸入手機 App 新產生的六位網銀認證碼。"}
            </span>
          </div>
        )}
        {activeNow && (
          <section className="card detail-block progress-block">
            <header>
              <strong>
                <Loader2 size={15} className="spin inline-icon" /> 同步進度
              </strong>
              <span className="dim small">
                開始於 {stamp(activeNow.created_at)}
              </span>
            </header>
            <SyncSteps job={activeNow} />
            <button
              type="button"
              className="link tl-toggle"
              aria-expanded={liveLog}
              onClick={() => setLiveLog(!liveLog)}
            >
              {liveLog ? "隱藏即時 log" : "顯示即時 log"}
            </button>
            {liveLog && <SyncLog job={activeNow} />}
          </section>
        )}
        {!activeNow && status.label === "同步失敗" && lastFailed && (
          <>
            {lastFailed.status === "failed" ? (
              <SyncErrorCard
                job={lastFailed}
                retryDisabled={!approved || !c.enabled || busy}
                onRetry={startSync}
                onEdit={() => setEditing(true)}
                onShowLog={() => setFailedLog(true)}
              />
            ) : (
              <div className="callout warn">
                <CircleX size={16} />
                <span>
                  {relative(lastFailed.created_at)}的同步沒有完成：
                  {lastFailed.message}
                  。可以再同步一次；若持續失敗，請更新登入設定。
                </span>
              </div>
            )}
            {failedLog && <SyncLog job={lastFailed} focusError />}
          </>
        )}
        {!c.configured && (
          <div className="callout">
            <CircleSlash size={16} />
            <span>
              已取消連結，先前同步的資料仍保留。可重新連結，或在下方移除資料來源以永久刪除紀錄。
            </span>
          </div>
        )}
        {paused ? (
          <div className="callout bad guard-callout">
            <ShieldAlert size={18} />
            <span>
              <strong>已連續 {failures} 次登入未成功，已暫停同步</strong>
              為了避免銀行鎖住帳號，自動與手動同步都先停止。請到銀行網站或 App
              確認帳號密碼與帳號狀態，修好後按「已修好，恢復同步」；如果密碼改過，直接「更新」登入設定也會恢復。
            </span>
            <Button
              size="sm"
              variant="primary"
              busy={busy}
              disabled={!approved}
              onClick={() =>
                run(`/connections/${c.id}/resume`, {
                  success: `已恢復${s.name}的同步`,
                  toastError: true,
                })
              }
            >
              已修好，恢復同步
            </Button>
          </div>
        ) : (
          failures > 0 &&
          c.configured && (
            <p className="hint-line warn-text">
              上次登入未成功（連續 {failures} 次，達 {limit}{" "}
              次會暫停同步）。登入成功後會歸零。
            </p>
          )
        )}

        <div className="source-grid">
          <div className="source-col">
            <dl className="source-facts">
              <div>
                <dt>上次同步</dt>
                <dd>
                  {c.last_success_at ? relative(c.last_success_at) : "尚未完成"}
                  {c.last_success_at && (
                    <small>{stamp(c.last_success_at)}</small>
                  )}
                </dd>
              </div>
              <div>
                <dt>{s.manual_only ? "同步方式" : "下次自動同步"}</dt>
                <dd>
                  {s.manual_only
                    ? "僅手動同步"
                    : paused
                      ? "已暫停"
                      : c.configured && c.enabled && c.daily && c.next_run_at
                        ? stamp(c.next_run_at)
                        : "未排程"}
                  <small>
                    {s.manual_only
                      ? s.id === "fubon"
                        ? "已偵測本人驗證要求，不再自動排程"
                        : "每次需輸入新的網銀認證碼"
                      : paused
                        ? "登入保護啟動中"
                        : c.configured && c.enabled && c.daily
                          ? `每天 ${sync?.times.join("、") ?? ""}（所有來源共用）`
                          : "需要時手動同步"}
                  </small>
                </dd>
              </div>
            </dl>
            <section className="card detail-block">
              <header>
                <strong>取得的資料</strong>
                {kinds.length > 0 && (
                  <button
                    type="button"
                    className="link"
                    onClick={() => setViewing(true)}
                  >
                    <Database size={14} /> 查看全部
                  </button>
                )}
              </header>
              {kinds.length ? (
                <div className="data-tiles">
                  {kinds.map((k) => (
                    <button
                      type="button"
                      key={k}
                      className="data-tile"
                      onClick={() => setViewing(k)}
                    >
                      <strong>
                        {counts[k].toLocaleString("zh-TW")}
                        <small> {kindInfo[k].unit}</small>
                      </strong>
                      <span>{kindInfo[k].label}</span>
                    </button>
                  ))}
                </div>
              ) : (
                <p className="hint-line">
                  第一次同步完成後，這裡會列出取得的資料。
                </p>
              )}
              <p className="hint-line">
                {s.id === "einvoice"
                  ? "發票用於查看消費內容，尚未與銀行交易配對；不直接計入淨資產或收支。"
                  : s.id === "tdcc"
                    ? "持倉會計入資產；集保銀行帳戶與定存僅供參考，可在「資產」頁連結對應銀行帳戶。"
                    : "帳戶、持股與信用卡會計入資產，可在「資產」頁逐項關閉。"}
              </p>
            </section>
            <section className="card detail-block">
              <header>
                <strong>同步紀錄</strong>
              </header>
              {history.length ? (
                <ol className="timeline compact">
                  {history.map((j) => (
                    <TimelineItem key={j.id} job={j} />
                  ))}
                </ol>
              ) : (
                <p className="hint-line">還沒有同步紀錄。</p>
              )}
            </section>
          </div>
          <div className="source-col">
            {s.id === "fubon" && c.configured && (
              <GmailVerification
                connection={c.id}
                disabled={!approved || syncing}
              />
            )}
            {(c.configured || c.record_count > 0) && (
              <section className="card detail-block">
                <header>
                  <strong>設定</strong>
                </header>
                <ul className="settings-list">
                  {c.configured && (
                    <>
                      {!s.manual_only && (
                        <li>
                          <span>
                            <strong>自動同步</strong>
                            <small>
                              依共用的同步時間
                              {sync ? `（每天 ${sync.times.join("、")}）` : ""}
                              自動更新。
                            </small>
                          </span>
                          <Switch
                            checked={c.enabled && c.daily}
                            label="每日自動同步"
                            disabled={busy || !approved || !c.enabled}
                            onChange={(v) =>
                              patch(
                                c.enabled,
                                v,
                                v ? "已開啟每日自動同步" : "已關閉每日自動同步",
                              )
                            }
                          />
                        </li>
                      )}
                      <li>
                        <span>
                          <strong>暫停這個來源</strong>
                          <small>
                            暫停後不會同步，已取得的資料與資產都會保留。
                          </small>
                        </span>
                        <Switch
                          checked={!c.enabled}
                          label="暫停這個來源"
                          disabled={busy || !approved}
                          onChange={(v) =>
                            patch(
                              !v,
                              !v ? c.daily : false,
                              v ? "已暫停" : "已恢復同步",
                            )
                          }
                        />
                      </li>
                      <li>
                        <span>
                          <strong>登入設定</strong>
                          <small>加密保存在這台主機，儲存後不會再顯示。</small>
                        </span>
                        <Button
                          size="sm"
                          icon={KeyRound}
                          disabled={!approved}
                          onClick={() => setEditing(true)}
                        >
                          更新
                        </Button>
                      </li>
                    </>
                  )}
                  <li>
                    <span>
                      <strong>移除資料來源</strong>
                      <small>
                        停止同步，永久刪除登入設定、帳戶、交易與資產歷史。
                      </small>
                    </span>
                    <Button
                      size="sm"
                      variant="danger-ghost"
                      icon={Trash2}
                      aria-label={`移除${s.name}及所有資料`}
                      title={`移除${s.name}及所有資料`}
                      disabled={busy}
                      onClick={() => setRemoving(true)}
                    >
                      移除資料來源
                    </Button>
                  </li>
                </ul>
              </section>
            )}
          </div>
        </div>
      </div>

      {viewing && (
        <SourceData
          connection={c}
          name={s.name}
          initialKind={viewing === true ? undefined : viewing}
          onClose={() => setViewing(undefined)}
        />
      )}
      {editing && (
        <CredentialSheet source={s} onClose={() => setEditing(false)} />
      )}
      {removing && (
        <Sheet
          title={`移除${s.name}及所有資料？`}
          size="sm"
          onClose={() => setRemoving(false)}
          busy={busy}
        >
          <div className="form">
            <p>
              會停止排程並取消同步，永久刪除此來源的登入設定、帳戶、持股、交易、餘額歷史與同步紀錄，資產及收支合計也會更新。此操作無法復原；重新連結後需要重新同步。
            </p>
            <div className="form-actions">
              <Button onClick={() => setRemoving(false)}>取消</Button>
              <Button
                variant="danger"
                busy={busy}
                onClick={async () => {
                  if (
                    await run(`/connections/${c.id}`, {
                      method: "DELETE",
                      success: "已移除資料來源及所有紀錄",
                      toastError: true,
                    })
                  )
                    onClose();
                }}
              >
                永久刪除所有資料
              </Button>
            </div>
          </div>
        </Sheet>
      )}
    </div>
  );
}

/* ---------------- 新增資料來源 ---------------- */
function AddSource({
  sources,
  onClose,
  onOpen,
}: {
  sources: Connector[];
  onClose: () => void;
  onOpen: (id: string) => void;
}) {
  const { run, busy, error } = useAction();
  const [picked, setPicked] = useState<Connector | undefined>(
    sources.length === 1 ? sources[0] : undefined,
  );
  const [daily, setDaily] = useState(true);
  const [done, setDone] = useState<string>();
  const [q, setQ] = useState("");
  const list = sources.filter((s) => !q || s.name.includes(q));
  const g = picked ? guide[picked.id] : undefined;

  const { user, toast } = useApp();
  const client = useQueryClient();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState("");
  async function saveCredentials(id: string, credentials: unknown) {
    setSaving(true);
    setSaveError("");
    try {
      const r = await api<{ id: string }>(`/connectors/${id}/credentials`, {
        method: "POST",
        body: { credentials },
        csrf: user.csrf_token,
      });
      await client.invalidateQueries();
      toast(`已加密儲存${picked?.name ?? ""}登入設定`);
      return r.id;
    } catch (err) {
      setSaveError((err as Error).message);
      return null;
    } finally {
      setSaving(false);
    }
  }

  if (done && picked)
    return (
      <Sheet title="已連結" onClose={onClose} size="sm">
        <div className="add-done">
          <SourceLogo id={picked.id} size={56} />
          <strong>{picked.name}已連結</strong>
          <p className="muted">
            {picked.manual_only
              ? "按下同步後，請輸入手機 App 新產生的網銀認證碼。"
              : "現在就同步一次，帶入你的資料。"}
          </p>
          <div className="form-actions center">
            <Button onClick={() => onOpen(picked.id)}>稍後再說</Button>
            <Button
              variant="primary"
              icon={Play}
              busy={busy}
              onClick={async () => {
                if (
                  await run(`/connections/${done}/sync`, {
                    success: `已開始同步${picked.name}`,
                    toastError: true,
                  })
                )
                  onOpen(picked.id);
              }}
            >
              立即同步
            </Button>
          </div>
        </div>
      </Sheet>
    );

  if (!picked)
    return (
      <Sheet
        title="新增資料來源"
        description="選擇要連結的銀行、證券或電子發票"
        onClose={onClose}
      >
        {sources.length > 6 && (
          <label className="search mb-12">
            <Search size={16} />
            <input
              type="search"
              aria-label="搜尋資料來源"
              placeholder="搜尋銀行或機構"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
          </label>
        )}
        {!list.length ? (
          <Empty compact title="沒有可以新增的來源" />
        ) : (
          <ul className="add-list">
            {list.map((s) => (
              <li key={s.id}>
                <button type="button" onClick={() => setPicked(s)}>
                  <SourceLogo id={s.id} />
                  <span>
                    <strong>
                      {s.name}
                      <em>{sourceType(s.id)}</em>
                    </strong>
                    <small>
                      {(guide[s.id]?.offers ?? s.capabilities)
                        .slice(0, 2)
                        .join("、")}
                    </small>
                    <small className="dim">
                      <>
                        <Zap size={12} className="inline-icon" /> 直接連線
                      </>
                    </small>
                  </span>
                  <ChevronRight size={18} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Sheet>
    );

  return (
    <Sheet
      title={
        <span className="detail-title">
          {sources.length > 1 && (
            <button
              type="button"
              className="icon-btn sm"
              aria-label="返回選擇來源"
              onClick={() => setPicked(undefined)}
            >
              <ArrowLeft size={16} />
            </button>
          )}
          連結{picked.name}
        </span>
      }
      description={`${sourceType(picked.id)} · 直接連線`}
      onClose={onClose}
      busy={busy}
    >
      <form
        className="form"
        autoComplete="off"
        onSubmit={async (e) => {
          e.preventDefault();
          const credentials = Object.fromEntries(new FormData(e.currentTarget));
          const saved = await saveCredentials(picked.id, credentials);
          if (!saved) return;
          if (!picked.manual_only && !daily)
            await run(`/connections/${saved}`, {
              method: "PATCH",
              body: { enabled: true, daily: false },
              toastError: true,
            });
          setDone(saved);
        }}
      >
        {g && (
          <div className="add-guide">
            <div>
              <strong>會帶入</strong>
              <ul className="check-list">
                {g.offers.map((o) => (
                  <li key={o}>
                    <CircleCheck size={16} />
                    {o}
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <strong>需要知道</strong>
              <ul className="note-list">
                {g.notes.map((n) => (
                  <li key={n}>{n}</li>
                ))}
              </ul>
            </div>
          </div>
        )}
        <FormError>{error || saveError}</FormError>
        {picked.credential_fields.map((f, i) => (
          <Field
            key={f}
            label={
              picked.id === "einvoice" && f === "password"
                ? "驗證碼（密碼）"
                : (fields[f] ?? f)
            }
          >
            <input
              name={f}
              type="password"
              autoComplete="new-password"
              maxLength={256}
              required
              data-autofocus={i === 0 || undefined}
            />
          </Field>
        ))}
        {!picked.manual_only && (
          <label className="switch-line spread">
            <span>
              <strong>每日自動同步</strong>
              <small>每天更新，持續補齊來源資料。</small>
            </span>
            <Switch checked={daily} label="每日自動同步" onChange={setDaily} />
          </label>
        )}
        <div className="callout good">
          <ShieldCheck size={18} />
          <span>
            登入資料只供這個來源使用，加密保存在這台主機，儲存後不會再顯示。
          </span>
        </div>
        <div className="form-actions">
          <Button onClick={onClose} disabled={busy || saving}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy || saving}>
            連結
          </Button>
        </div>
      </form>
    </Sheet>
  );
}

/* ---------------- 同步紀錄 ---------------- */
/** 同步階段清單：已完成打勾、進行中轉圈，附相對開始的時間 */
function SyncSteps({ job }: { job: Job }) {
  const steps = job.progress ?? [];
  const running = activeJob(job.status);
  const start = Date.parse(job.created_at);
  const elapsed = (at: string) => {
    const s = Math.max(0, Math.round((Date.parse(at) - start) / 1000));
    return s < 60 ? `${s} 秒` : `${Math.floor(s / 60)} 分 ${s % 60} 秒`;
  };
  if (!steps.length)
    return (
      <p className="hint-line">
        <Loader2 size={13} className="spin inline-icon" /> {job.message}
      </p>
    );
  return (
    <ol className="sync-steps">
      {steps.map((st, i) => {
        const last = i === steps.length - 1;
        const waiting = last && job.status === "waiting_user";
        const active = last && running && !waiting;
        const pct =
          st.total && st.current != null
            ? Math.round((st.current / st.total) * 100)
            : null;
        return (
          <li
            key={`${st.code}-${i}`}
            className={active ? "active" : waiting ? "waiting" : "done"}
          >
            <span className="ss-icon">
              {active ? (
                <Loader2 size={14} className="spin" />
              ) : waiting ? (
                <ShieldAlert size={14} />
              ) : (
                <CircleCheck size={14} />
              )}
            </span>
            <span className="ss-main">
              <span>
                {st.label}
                {st.total ? `（${st.current ?? 0}/${st.total}）` : ""}
              </span>
              {active && pct != null && (
                <span className="ss-bar">
                  <span style={{ width: `${pct}%` }} />
                </span>
              )}
            </span>
            <small>{elapsed(st.at)}</small>
          </li>
        );
      })}
    </ol>
  );
}

function TimelineItem({ job: j }: { job: Job }) {
  const [open, setOpen] = useState(false);
  const [log, setLog] = useState(false);
  const count = j.summary
    ? Object.values(j.summary).reduce((a, b) => a + b, 0)
    : null;
  const view =
    j.status === "succeeded"
      ? { icon: CircleCheck, tone: "good", title: "同步完成" }
      : j.status === "partial"
        ? { icon: CircleCheck, tone: "warn", title: "部分完成" }
        : j.status === "failed"
          ? { icon: CircleX, tone: "bad", title: "同步失敗" }
          : { icon: CircleSlash, tone: "neutral", title: "已取消" };
  return (
    <li className={`tl-item ${view.tone}`}>
      <span className="tl-dot">
        <view.icon size={16} />
      </span>
      <span className="tl-main">
        <strong>
          {view.title}
          <small> · {sourceNames[j.connector ?? ""] ?? j.connector}</small>
        </strong>
        <small>
          {count != null && j.status !== "failed"
            ? `取得 ${count.toLocaleString("zh-TW")} 筆資料`
            : j.message}
          {!!j.progress?.length && (
            <button
              type="button"
              className="link tl-toggle"
              onClick={() => setOpen(!open)}
            >
              {open ? "收合步驟" : `${j.progress.length} 個步驟`}
            </button>
          )}
          <button
            type="button"
            className="link tl-toggle"
            aria-expanded={log}
            onClick={() => setLog(!log)}
          >
            {log ? "收合 log" : "查看 log"}
          </button>
        </small>
        {open && <SyncSteps job={j} />}
        {log && <SyncLog job={j} focusError={j.status === "failed"} />}
      </span>
      <span className="tl-time">
        {stamp(j.created_at)}
        <small>{relative(j.created_at)}</small>
      </span>
    </li>
  );
}

/* ---------------- 進階 ---------------- */
function Advanced({
  health,
  approved,
  isAdmin,
}: {
  health?: Health;
  approved: boolean;
  isAdmin: boolean;
}) {
  return (
    <details className="card advanced">
      <summary>
        <span>
          <strong>進階</strong>
          <small>系統狀態{isAdmin ? "、成員權限" : ""}</small>
        </span>
        <ChevronDown size={18} />
      </summary>
      <div className="advanced-body">
        <div className="advanced-row">
          <strong>系統狀態</strong>
          <ul className="status-list">
            <li>
              <i className={health?.database ? "ok" : "bad"} /> 資料庫
              {health?.database ? "正常" : "未連線"}
            </li>
            <li>
              <i className={health?.redis ? "ok" : "bad"} /> 工作佇列
              {health?.redis ? "正常" : "未連線"}
            </li>
            <li>
              <i className={approved ? "ok" : "warn"} />
              {approved ? "你可以使用同步" : "同步資格待核准"}
            </li>
          </ul>
        </div>
        {isAdmin && (
          <div className="advanced-row">
            <strong>成員權限</strong>
            <p className="hint-line">
              核准同步資格、指定管理員已移到獨立的「成員」頁。{" "}
              <a className="link" href="#/members">
                前往成員管理
              </a>
            </p>
          </div>
        )}
      </div>
    </details>
  );
}

/* ---------------- 資料檢視 ---------------- */
function SourceData({
  connection,
  name,
  initialKind,
  onClose,
}: {
  connection: Connection;
  name: string;
  initialKind?: string;
  onClose: () => void;
}) {
  const counts = connection.counts ?? {};
  const kinds = kindOrder.filter((k) => counts[k]);
  const [kind, setKind] = useState(initialKind ?? kinds[0] ?? "");
  const [q, setQ] = useState("");
  const [offset, setOffset] = useState(0);
  const [invoice, setInvoice] = useState<SourceRecord>();
  const query = useDebounced(q.trim(), 300);
  const result = useQuery({
    queryKey: ["source-records", connection.id, kind, query, offset],
    queryFn: () =>
      api<{
        total: number;
        rows: SourceRecord[];
        invoice_totals?: { currency: string; amount: string; count: number }[];
      }>(
        `/connections/${connection.id}/records?offset=${offset}${kind ? `&kind=${kind}` : ""}${query ? `&q=${encodeURIComponent(query)}` : ""}`,
      ),
  });
  const total = result.data?.total ?? 0;
  return (
    <Sheet
      title={`${name}的資料`}
      description="同步帶回來的內容；缺少的欄位顯示「—」"
      side="right"
      size="lg"
      onClose={onClose}
    >
      <div className="data-tabs" role="tablist" aria-label="資料類型">
        {kinds.map((k) => (
          <button
            type="button"
            role="tab"
            key={k}
            aria-selected={kind === k}
            className={kind === k ? "on" : ""}
            onClick={() => {
              setKind(k);
              setOffset(0);
            }}
          >
            {kindInfo[k].label}
            <small>{counts[k].toLocaleString("zh-TW")}</small>
          </button>
        ))}
      </div>
      <label className="search mb-12">
        <Search size={16} />
        <input
          type="search"
          aria-label="搜尋來源資料"
          placeholder="搜尋名稱、代號或說明"
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setOffset(0);
          }}
        />
      </label>
      {result.isError && <ErrorLine>資料讀取失敗</ErrorLine>}
      {kind === "invoice" && result.data && (
        <div className="callout info">
          <ReceiptText size={18} />
          <span>
            有效發票{" "}
            {result.data.invoice_totals
              ?.map((t) => `${t.count} 張 · ${money(t.amount, t.currency)}`)
              .join("；") || "0 張"}
            。作廢發票不計入；發票金額含捐贈，尚未扣除折讓或與銀行消費配對。
          </span>
        </div>
      )}
      {result.isPending ? (
        <Skeleton rows={5} h={52} />
      ) : !result.data?.rows.length ? (
        <Empty compact title="沒有符合條件的資料" />
      ) : (
        <RecordTable
          kind={kind}
          rows={result.data.rows}
          onInvoice={setInvoice}
        />
      )}
      {total > 100 && (
        <div className="pager">
          <Button
            size="sm"
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - 100))}
          >
            上一頁
          </Button>
          <span>
            共 {total.toLocaleString("zh-TW")} 筆 · 第{" "}
            {Math.floor(offset / 100) + 1} / {Math.ceil(total / 100)} 頁
          </span>
          <Button
            size="sm"
            disabled={offset + 100 >= total}
            onClick={() => setOffset(offset + 100)}
          >
            下一頁
          </Button>
        </div>
      )}
      {invoice && (
        <InvoiceItems
          connectionId={connection.id}
          invoice={invoice}
          onClose={() => setInvoice(undefined)}
        />
      )}
    </Sheet>
  );
}

const str = (v: unknown) =>
  v === null || v === undefined || v === "" ? null : String(v);
const amt = (v: unknown, currency: unknown) => {
  const s = str(v);
  return s == null ? "—" : money(s, str(currency) ?? "TWD");
};
const day = (v: unknown) => {
  const s = str(v);
  return s ? s.slice(0, 10).replaceAll("-", "/") : "—";
};
const assetTypes: Record<string, string> = {
  total: "合計",
  stock: "股票",
  fund: "基金",
  etf: "ETF",
  bond: "債券",
};

/** 依資料類型挑出有意義的欄位，而不是把原始欄位全部攤開 */
function RecordTable({
  kind,
  rows,
  onInvoice,
}: {
  kind: string;
  rows: SourceRecord[];
  onInvoice?: (invoice: SourceRecord) => void;
}) {
  const cell = (label: string, value: ReactNode, cls = "") => (
    <span className={`rt-cell ${cls}`} data-label={label}>
      {value}
    </span>
  );
  const heads: Record<string, string[]> = {
    account: ["帳戶", "銀行", "餘額", "資料日期"],
    card: ["卡片", "末四碼", "類型", "狀態"],
    holding: ["名稱", "數量", "市值", "資料日期"],
    investment_transaction: ["日期", "名稱", "數量／價格", "金額"],
    transaction: ["日期", "說明", "帳戶", "金額"],
    net_worth: ["日期", "類別", "金額", ""],
    invoice: ["日期", "店家／發票", "載具／狀態", "金額"],
    invoice_item: ["發票", "品名", "數量／單價", "金額"],
  };
  const head = heads[kind];
  if (!head)
    return (
      <div className="records">
        {rows.map((r) => (
          <article className="source-record" key={r.id}>
            <dl>
              {Object.entries(r.data)
                .filter(([, v]) => v !== null && v !== "")
                .map(([k, v]) => (
                  <div key={k}>
                    <dt>{k}</dt>
                    <dd>{String(v)}</dd>
                  </div>
                ))}
            </dl>
          </article>
        ))}
      </div>
    );
  return (
    <div className="rtable" role="table">
      <div className="rt-row rt-head" role="row">
        {head.map((h, i) => (
          <span key={i} className="rt-cell" role="columnheader">
            {h}
          </span>
        ))}
      </div>
      {rows.map((r) => {
        const d = r.data;
        let cells: ReactNode;
        if (kind === "account")
          cells = (
            <>
              {cell(
                "帳戶",
                <>
                  <strong>{str(d.name) ?? "—"}</strong>
                  <small>{str(d.mask) ?? ""}</small>
                </>,
              )}
              {cell("銀行", str(d.institution) ?? "—")}
              {cell("餘額", amt(d.balance, d.currency), "num right")}
              {cell("資料日期", day(d.as_of), "dim")}
            </>
          );
        else if (kind === "invoice")
          cells = (
            <>
              {cell("日期", day(d.date), "dim")}
              {cell(
                "店家／發票",
                <>
                  <button
                    type="button"
                    className="link"
                    onClick={() => onInvoice?.(r)}
                  >
                    {str(d.seller) || "未知店家"}
                  </button>
                  <small>{str(d.number)}</small>
                </>,
              )}
              {cell(
                "載具／狀態",
                <>
                  {str(d.carrier) || "—"}
                  <small>
                    {d.status === "void"
                      ? "已作廢"
                      : d.donated
                        ? "已捐贈"
                        : "有效發票"}
                    {d.detail_status === "missing" ? " · 品項待同步" : ""}
                    {d.has_allowance ? " · 有折讓" : ""}
                  </small>
                </>,
              )}
              {cell("金額", amt(d.amount, d.currency), "num right")}
            </>
          );
        else if (kind === "invoice_item")
          cells = (
            <>
              {cell("發票", r.invoice_number || "—", "dim")}
              {cell("品名", str(d.description) || "—")}
              {cell(
                "數量／單價",
                <>
                  {str(d.quantity) ?? "—"}
                  <small>@ {amt(d.unit_price, d.currency)}</small>
                </>,
                "num right",
              )}
              {cell("金額", amt(d.amount, d.currency), "num right")}
            </>
          );
        else if (kind === "card")
          cells = (
            <>
              {cell("卡片", str(d.name) ?? "—")}
              {cell("末四碼", str(d.mask) ?? "—")}
              {cell("類型", str(d.card_type) ?? "—")}
              {cell("狀態", str(d.status) ?? "—")}
            </>
          );
        else if (kind === "holding")
          cells = (
            <>
              {cell(
                "名稱",
                <>
                  <strong>{str(d.name) ?? "—"}</strong>
                  <small>
                    {[str(d.symbol), assetTypes[str(d.asset_type) ?? ""]]
                      .filter(Boolean)
                      .join(" · ")}
                  </small>
                </>,
              )}
              {cell(
                "數量",
                d.quantity
                  ? `${quantity(String(d.quantity))} ${d.asset_type === "fund" ? "單位" : "股"}`
                  : "—",
                "num right",
              )}
              {cell("市值", amt(d.market_value, d.currency), "num right")}
              {cell("資料日期", day(d.as_of), "dim")}
            </>
          );
        else if (kind === "investment_transaction")
          cells = (
            <>
              {cell("日期", day(d.date), "dim")}
              {cell(
                "名稱",
                <>
                  <strong>{str(d.name) ?? "—"}</strong>
                  <small>
                    {[str(d.symbol), str(d.description)]
                      .filter(Boolean)
                      .join(" · ")}
                  </small>
                </>,
              )}
              {cell(
                "數量／價格",
                <>
                  {d.quantity ? `${quantity(String(d.quantity))} 股` : "—"}
                  <small>
                    {str(d.price)
                      ? `@ ${new Decimal(String(d.price)).toString()}`
                      : ""}
                  </small>
                </>,
                "num right",
              )}
              {cell("金額", amt(d.amount, d.currency), "num right")}
            </>
          );
        else if (kind === "transaction") {
          const a = str(d.amount);
          cells = (
            <>
              {cell("日期", day(d.date), "dim")}
              {cell(
                "說明",
                <>
                  <strong>{str(d.description) ?? "—"}</strong>
                  {d.status === "pending" && <small>未入帳</small>}
                </>,
              )}
              {cell("帳戶", r.account_name ?? "—", "dim")}
              {cell(
                "金額",
                a == null ? "—" : signed(a, str(d.currency) ?? "TWD"),
                `num right ${a && new Decimal(a).isPositive() ? "pos" : ""}`,
              )}
            </>
          );
        } else
          cells = (
            <>
              {cell("日期", day(d.date), "dim")}
              {cell(
                "類別",
                assetTypes[str(d.asset_type) ?? ""] ?? str(d.asset_type) ?? "—",
              )}
              {cell("金額", amt(d.amount, d.currency), "num right")}
              {cell("", "")}
            </>
          );
        return (
          <div
            className={`rt-row ${r.current ? "" : "stale"}`}
            role="row"
            key={r.id}
          >
            {cells}
          </div>
        );
      })}
    </div>
  );
}

function InvoiceItems({
  connectionId,
  invoice,
  onClose,
}: {
  connectionId: string;
  invoice: SourceRecord;
  onClose: () => void;
}) {
  const [offset, setOffset] = useState(0);
  const result = useQuery({
    queryKey: ["invoice-items", connectionId, invoice.id, offset],
    queryFn: () =>
      api<{ total: number; rows: SourceRecord[] }>(
        `/connections/${connectionId}/records?kind=invoice_item&invoice=${invoice.id}&offset=${offset}`,
      ),
  });
  return (
    <Sheet
      title={`${str(invoice.data.seller) || "發票"} · ${str(invoice.data.number) || ""}`}
      description={`${day(invoice.data.date)} · ${amt(invoice.data.amount, invoice.data.currency)}`}
      onClose={onClose}
      side="right"
      size="lg"
    >
      {invoice.data.status === "void" && (
        <p className="callout warn">此發票已作廢。</p>
      )}
      {invoice.data.detail_status === "missing" && (
        <p className="callout warn">品項尚未同步完成，下次同步會重新取得。</p>
      )}
      {invoice.data.has_allowance && (
        <p className="callout warn">
          此發票有折讓；目前顯示原發票金額，折讓明細尚未整合。
        </p>
      )}
      {result.isPending ? (
        <Skeleton rows={3} h={52} />
      ) : result.isError ? (
        <ErrorLine>品項讀取失敗</ErrorLine>
      ) : result.data.rows.length ? (
        <RecordTable kind="invoice_item" rows={result.data.rows} />
      ) : (
        <Empty compact title="沒有可顯示的品項" />
      )}
      {(result.data?.total ?? 0) > 100 && (
        <div className="pager">
          <Button
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - 100))}
          >
            上一頁
          </Button>
          <span>共 {result.data?.total} 項</span>
          <Button
            disabled={offset + 100 >= (result.data?.total ?? 0)}
            onClick={() => setOffset(offset + 100)}
          >
            下一頁
          </Button>
        </div>
      )}
    </Sheet>
  );
}

/* 任何頁面都會跳出的本人驗證視窗；看 log 或在別頁時也不會漏掉 */
export function ChallengePrompt() {
  const { jobs } = useJobsContext();
  const { viewing } = useApp();
  const [later, setLater] = useState<Set<string>>(new Set());
  const pending = jobs.filter(
    (j) =>
      j.status === "waiting_user" &&
      !j.answer_pending &&
      j.challenge &&
      !later.has(j.challenge.id),
  );
  const count = jobs.filter(
    (j) => j.status === "waiting_user" && !j.answer_pending,
  ).length;
  useEffect(() => {
    if (!count) return;
    const title = document.title;
    document.title = `🔐 需要驗證 · ${title}`;
    navigator.vibrate?.(200);
    return () => {
      document.title = title;
    };
  }, [count]);
  const j = pending[0];
  if (viewing || !j) return null;
  return (
    <Sheet
      title={
        <span className="detail-title">
          <ShieldAlert size={18} /> 需要你驗證
        </span>
      }
      description="同步正在等你輸入，請在期限內完成"
      size="sm"
      onClose={() => setLater(new Set(later).add(j.challenge!.id))}
    >
      <Challenge job={j} />
      <button
        type="button"
        className="link challenge-later"
        onClick={() => setLater(new Set(later).add(j.challenge!.id))}
      >
        稍後再說（可在「資料來源」頁完成）
      </button>
    </Sheet>
  );
}

function Challenge({ job: j }: { job: Job }) {
  const { run, busy, error } = useAction();
  const [value, setValue] = useState("");
  useEffect(() => setValue(""), [j.challenge?.id]);
  const name = sourceNames[j.connector ?? ""] ?? "資料來源";
  const cancel = () =>
    run(`/sync/jobs/${j.id}/cancel`, { success: "已取消", toastError: true });
  if (j.answer_pending || !j.challenge)
    return (
      <div className="challenge">
        <strong>{name}</strong>
        <p className="muted">驗證資料已送出，等待來源繼續…</p>
        <div className="challenge-row">
          <Button variant="ghost" icon={X} disabled={busy} onClick={cancel}>
            取消同步
          </Button>
        </div>
      </div>
    );
  const choice = j.challenge.kind === "choice";
  return (
    <form
      className="challenge"
      onSubmit={async (e: FormEvent) => {
        e.preventDefault();
        if (
          await run(`/sync/jobs/${j.id}/continue`, {
            body: { challenge_id: j.challenge!.id, value },
            success: "已送出，等待來源繼續",
          })
        )
          setValue("");
      }}
    >
      <strong>{name}</strong>
      <p>{j.challenge.message}</p>
      {j.challenge.expires_at && (
        <p className="muted">
          請於{" "}
          {new Date(j.challenge.expires_at).toLocaleTimeString("zh-TW", {
            hour12: false,
          })}{" "}
          前送出；過期後請取消並重新同步。
        </p>
      )}
      {choice && (
        <fieldset className="verification-choices">
          <legend>請選擇</legend>
          {j.challenge.choices?.map((option) => (
            <label key={option.value}>
              <input
                type="radio"
                name="verification-choice"
                value={option.value}
                checked={value === option.value}
                onChange={() => setValue(option.value)}
                required
                disabled={busy}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </fieldset>
      )}
      {j.challenge.image && (
        <img
          src={j.challenge.image}
          alt="來源圖片驗證碼"
          style={{ background: "white" }}
          referrerPolicy="no-referrer"
        />
      )}
      <FormError>{error}</FormError>
      <div className="challenge-row">
        {!choice && (
          <input
            aria-label="本人驗證碼"
            autoComplete="one-time-code"
            inputMode="text"
            maxLength={32}
            required
            autoFocus
            placeholder="輸入驗證碼"
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
        )}
        <Button type="submit" variant="primary" busy={busy}>
          {choice ? "確認選擇" : "送出驗證碼"}
        </Button>
        <Button variant="ghost" disabled={busy} onClick={cancel}>
          取消同步
        </Button>
      </div>
    </form>
  );
}

function CredentialSheet({
  source,
  onClose,
}: {
  source: Connector;
  onClose: () => void;
}) {
  const { run, busy, error } = useAction();
  return (
    <Sheet title={`${source.name}登入設定`} onClose={onClose} busy={busy}>
      <form
        className="form"
        autoComplete="off"
        onSubmit={async (e) => {
          e.preventDefault();
          const credentials = Object.fromEntries(new FormData(e.currentTarget));
          if (
            await run(`/connectors/${source.id}/credentials`, {
              body: { credentials },
              success: `已加密儲存${source.name}登入設定`,
            })
          )
            onClose();
        }}
      >
        <div className="callout good">
          <ShieldCheck size={18} />
          <span>
            只供這個來源使用，加密保存在伺服器主機；儲存後不會再顯示。
          </span>
        </div>
        <FormError>{error}</FormError>
        {source.credential_fields.map((f, i) => (
          <Field
            key={f}
            label={
              source.id === "einvoice" && f === "password"
                ? "驗證碼（密碼）"
                : (fields[f] ?? f)
            }
          >
            <input
              name={f}
              type="password"
              autoComplete="new-password"
              maxLength={256}
              required
              data-autofocus={i === 0 || undefined}
            />
          </Field>
        ))}
        <ul className="hint-list">
          <li>首次設定會預設每日自動同步，可隨時關閉。</li>
          <li>更新設定會取消進行中的同步。</li>
        </ul>
        <div className="form-actions">
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            加密儲存
          </Button>
        </div>
      </form>
    </Sheet>
  );
}

/** 檢視家人帳本時只能看；這頁的操作都屬於帳本本人 */
function ReadOnlyNotice({ what }: { what: string }) {
  const { viewing, setViewing } = useApp();
  return (
    <Empty icon={BookOpen} title={`${what}只有帳本本人可以管理`}>
      正在唯讀查看 {viewing?.name} 的帳本。
      <button type="button" className="link" onClick={() => setViewing(null)}>
        切回我的帳本
      </button>
    </Empty>
  );
}
