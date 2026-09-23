"use client";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeftRight,
  ArrowRight,
  Bell,
  CalendarDays,
  BookOpen,
  Check,
  ChevronsUpDown,
  Ellipsis,
  Eye,
  EyeOff,
  LayoutDashboard,
  LogOut,
  Monitor,
  Moon,
  PlugZap,
  Plus,
  Repeat,
  ShieldAlert,
  Sun,
  Target,
  ChartPie,
  UserCog,
  HeartHandshake,
  Wallet,
  type LucideIcon,
} from "lucide-react";
import { api, type Health, type Notification, type User } from "@/lib/api";
import { relative, sourceNames } from "@/lib/format";
import { JobsContext, useJobs, useJobsContext } from "@/lib/queries";
import { CategoriesProvider } from "@/lib/categories";
import {
  AppProvider,
  readStore,
  useApp,
  useHotkey,
  writeStore,
  type ComposeOptions,
  type View,
} from "@/components/app-context";
import { Empty, Sheet, Writable } from "@/components/ui";
import { Composer } from "./composer";
import { OverviewPage } from "./overview";
import { ActivityPage } from "./activity";
import { AssetsPage } from "./assets";
import { AnalysisPage } from "./analysis";
import { MembersPage } from "./members";
import { ProfileSheet } from "./profile";
import { Avatar } from "@/components/avatar";
import { PlansPage } from "./plans";
import { HouseholdPage, useHousehold } from "./household";
import { ChallengePrompt, SourcesPage } from "./sources";
import { CalendarPage } from "./calendar";
import { SubscriptionsPage } from "./subscriptions";

const nav: { id: View; label: string; icon: LucideIcon }[] = [
  { id: "overview", label: "總覽", icon: LayoutDashboard },
  { id: "activity", label: "收支", icon: ArrowLeftRight },
  { id: "assets", label: "資產", icon: Wallet },
  { id: "analysis", label: "分析", icon: ChartPie },
  { id: "plans", label: "計畫", icon: Target },
  { id: "subscriptions", label: "訂閱", icon: Repeat },
  { id: "calendar", label: "行事曆", icon: CalendarDays },
  { id: "household", label: "家庭", icon: HeartHandshake },
  { id: "sources", label: "資料來源", icon: PlugZap },
];
/** 只有管理員看得到的頁面 */
const adminNav: (typeof nav)[number] = {
  id: "members",
  label: "成員",
  icon: UserCog,
};
const titles = Object.fromEntries(
  [...nav, adminNav].map((n) => [n.id, n.label]),
) as Record<View, string>;

export function Workspace({ user, health }: { user: User; health?: Health }) {
  const [composing, setComposing] = useState<ComposeOptions | null>(null);
  return (
    <AppProvider user={user} onCompose={(o) => setComposing(o ?? {})}>
      <CategoriesProvider>
        <Viewing>
          {(viewing) => (
            <Jobs approved={!viewing && (user.is_admin || user.sync_approved)}>
              <Shell health={health} />
            </Jobs>
          )}
        </Viewing>
        {composing && (
          <Composer options={composing} onClose={() => setComposing(null)} />
        )}
      </CategoriesProvider>
    </AppProvider>
  );
}

/** 還原上次查看的家庭成員（確認對方仍分享給我才套用） */
function Viewing({ children }: { children: (viewing: boolean) => ReactNode }) {
  const { viewing, setViewing } = useApp();
  const home = useHousehold();
  const [restored, setRestored] = useState(false);
  useEffect(() => {
    if (!home.data) return;
    const shared = home.data.shared_with_me;
    if (!restored) {
      const id = readStore<string | null>("viewing", null);
      const p = shared.find((x) => x.id === id);
      if (p) setViewing(p);
      else if (id) writeStore("viewing", null);
      setRestored(true);
    } else if (viewing && !shared.some((x) => x.id === viewing.id)) {
      setViewing(null);
    }
  }, [home.data, restored, viewing, setViewing]);
  return <>{children(!!viewing)}</>;
}

function Jobs({
  approved,
  children,
}: {
  approved: boolean;
  children: ReactNode;
}) {
  const jobs = useJobs(approved);
  return (
    <JobsContext.Provider
      value={{ jobs: jobs.data ?? [], live: jobs.live, error: jobs.isError }}
    >
      {children}
    </JobsContext.Provider>
  );
}

type Theme = "system" | "light" | "dark";
function useTheme() {
  const [theme, setTheme] = useState<Theme>("system");
  useEffect(() => setTheme(readStore<Theme>("theme", "system")), []);
  const change = (t: Theme) => {
    setTheme(t);
    writeStore("theme", t);
    if (t === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = t;
  };
  return [theme, change] as const;
}

function Shell({ health }: { health?: Health }) {
  const {
    user,
    route,
    navigate,
    compose,
    hidden,
    setHidden,
    viewing,
    setViewing,
  } = useApp();
  const home = useHousehold();
  const books = home.data?.shared_with_me ?? [];
  const [switching, setSwitching] = useState(false);
  const client = useQueryClient();
  const [theme, setTheme] = useTheme();
  const [notifying, setNotifying] = useState(false);
  const [more, setMore] = useState(false);
  const [editingProfile, setEditingProfile] = useState(false);
  const items = user.is_admin ? [...nav, adminNav] : nav;
  const { jobs: allJobs } = useJobsContext();
  const jobs = useMemo(
    () =>
      allJobs.filter((j) => j.status === "waiting_user" && !j.answer_pending),
    [allJobs],
  );
  useHotkey("n", () => compose());
  const view = route.view;
  const waiting = jobs.length;
  async function logout() {
    try {
      await api("/auth/logout", { method: "POST", csrf: user.csrf_token });
    } finally {
      client.clear();
      location.href = "/";
    }
  }
  const ThemeIcon = theme === "dark" ? Moon : theme === "light" ? Sun : Monitor;
  const nextTheme: Record<Theme, Theme> = {
    system: "light",
    light: "dark",
    dark: "system",
  };
  const themeLabel: Record<Theme, string> = {
    system: "跟隨系統",
    light: "淺色",
    dark: "深色",
  };

  return (
    <div className="shell">
      <a href="#main" className="skip">
        跳到主要內容
      </a>
      <aside className="sidebar" aria-label="主選單">
        <a className="brand" href="#/overview">
          <Logo />
          <span>Money Track</span>
        </a>
        {books.length > 0 && (
          <button
            type="button"
            className="book-switch"
            onClick={() => setSwitching(true)}
          >
            <Avatar person={viewing ?? user} />
            <span>
              <small>正在看</small>
              <strong>{viewing ? `${viewing.name} 的帳本` : "我的帳本"}</strong>
            </span>
            <ChevronsUpDown size={16} className="dim" />
          </button>
        )}
        <Writable>
          <button
            type="button"
            className="btn btn-primary compose-btn"
            onClick={() => compose()}
          >
            <Plus size={18} />
            記一筆
            <kbd>N</kbd>
          </button>
        </Writable>
        <nav>
          {items.map((n) => (
            <a
              key={n.id}
              href={`#/${n.id}`}
              className={`nav-item ${view === n.id ? "on" : ""}`}
              aria-current={view === n.id ? "page" : undefined}
            >
              <n.icon size={19} />
              <span>{n.label}</span>
              {n.id === "sources" && waiting > 0 && (
                <span className="nav-badge">{waiting}</span>
              )}
            </a>
          ))}
        </nav>
        <div className="sidebar-tools">
          <button
            type="button"
            className="tool"
            onClick={() => setHidden(!hidden)}
            aria-pressed={hidden}
          >
            {hidden ? <EyeOff size={17} /> : <Eye size={17} />}
            <span>{hidden ? "顯示金額" : "隱藏金額"}</span>
          </button>
          <button
            type="button"
            className="tool"
            onClick={() => setNotifying(true)}
          >
            <Bell size={17} />
            <span>通知</span>
          </button>
          <button
            type="button"
            className="tool"
            onClick={() => setTheme(nextTheme[theme])}
            title="切換外觀"
          >
            <ThemeIcon size={17} />
            <span>外觀：{themeLabel[theme]}</span>
          </button>
        </div>
        <div className="profile">
          <button
            type="button"
            className="profile-open"
            title="編輯個人資料"
            onClick={() => setEditingProfile(true)}
          >
            <Avatar person={user} />
            <span className="profile-text">
              <strong>{user.name}</strong>
              <small>{user.email}</small>
            </span>
          </button>
          <button
            type="button"
            className="icon-btn sm"
            title="登出"
            aria-label="登出"
            onClick={logout}
          >
            <LogOut size={16} />
          </button>
        </div>
      </aside>

      <header className="appbar">
        <a className="brand" href="#/overview" aria-label="Money Track 總覽">
          <Logo />
        </a>
        <strong>{titles[view]}</strong>
        <span className="grow" />
        {books.length > 0 && (
          <button
            type="button"
            className="icon-btn"
            aria-label="切換帳本"
            onClick={() => setSwitching(true)}
          >
            <Avatar person={viewing ?? user} size={28} />
          </button>
        )}
        <button
          type="button"
          className="icon-btn"
          aria-label={hidden ? "顯示金額" : "隱藏金額"}
          aria-pressed={hidden}
          onClick={() => setHidden(!hidden)}
        >
          {hidden ? <EyeOff size={19} /> : <Eye size={19} />}
        </button>
        <button
          type="button"
          className="icon-btn"
          aria-label="通知"
          onClick={() => setNotifying(true)}
        >
          <Bell size={19} />
        </button>
      </header>

      <main id="main" className="main" tabIndex={-1}>
        {viewing && (
          <div className="viewing-bar" role="status">
            <BookOpen size={18} />
            <span>
              正在查看 <strong>{viewing.name}</strong> 的帳本，只能看不能改
            </span>
            <button
              type="button"
              className="link"
              onClick={() => setViewing(null)}
            >
              回到我的帳本
            </button>
          </div>
        )}
        {waiting > 0 && view !== "sources" && view !== "overview" && (
          <div className="banner" role="alert">
            <ShieldAlert size={18} />
            <span>
              {sourceNames[jobs[0].connector ?? ""] ?? "資料來源"}
              {waiting > 1 ? ` 等 ${waiting} 個來源` : ""}正在等你輸入驗證碼
            </span>
            <button
              type="button"
              className="link"
              onClick={() => navigate("sources")}
            >
              前往驗證 <ArrowRight size={14} />
            </button>
          </div>
        )}
        <div className="page" key={view}>
          {view === "overview" && <OverviewPage />}
          {view === "activity" && <ActivityPage />}
          {view === "assets" && <AssetsPage />}
          {view === "analysis" && <AnalysisPage />}
          {view === "plans" && <PlansPage />}
          {view === "subscriptions" && <SubscriptionsPage />}
          {view === "calendar" && <CalendarPage />}
          {view === "household" && <HouseholdPage />}
          {view === "sources" && <SourcesPage health={health} />}
          {view === "members" && <MembersPage />}
        </div>
      </main>
      <ChallengePrompt />

      <nav className="tabbar" aria-label="主選單">
        {nav.slice(0, 2).map((n) => (
          <TabLink key={n.id} item={n} active={view === n.id} />
        ))}
        <button
          type="button"
          className="tab-compose"
          aria-label="記一筆"
          disabled={!!viewing}
          onClick={() => compose()}
        >
          <Plus size={26} />
        </button>
        <TabLink item={nav[2]} active={view === "assets"} />
        <button
          type="button"
          className={`tab ${["analysis", "plans", "subscriptions", "calendar", "household", "sources", "members"].includes(view) ? "on" : ""}`}
          onClick={() => setMore(true)}
        >
          <span className="tab-icon">
            <Ellipsis size={22} />
            {waiting > 0 && <span className="dot" />}
          </span>
          <span>更多</span>
        </button>
      </nav>

      {more && (
        <Sheet title="更多" onClose={() => setMore(false)} size="sm">
          <div className="more-grid">
            {items.slice(3).map((n) => (
              <button
                type="button"
                key={n.id}
                className={view === n.id ? "on" : ""}
                onClick={() => {
                  setMore(false);
                  navigate(n.id);
                }}
              >
                <n.icon size={22} />
                <span>{n.label}</span>
                {n.id === "sources" && waiting > 0 && (
                  <span className="nav-badge">{waiting}</span>
                )}
              </button>
            ))}
          </div>
          <div className="action-list">
            <button
              type="button"
              onClick={() => {
                setMore(false);
                setEditingProfile(true);
              }}
            >
              <Avatar person={user} size={22} />
              <span>
                個人資料
                <small>名稱與頭像</small>
              </span>
            </button>
            <button type="button" onClick={() => setTheme(nextTheme[theme])}>
              <ThemeIcon size={18} />
              <span>外觀：{themeLabel[theme]}</span>
            </button>
            <button type="button" onClick={logout}>
              <LogOut size={18} />
              <span>
                登出
                <small>{user.email}</small>
              </span>
            </button>
          </div>
        </Sheet>
      )}
      {notifying && <Notifications onClose={() => setNotifying(false)} />}
      {editingProfile && (
        <ProfileSheet onClose={() => setEditingProfile(false)} />
      )}
      {switching && (
        <Sheet title="切換帳本" size="sm" onClose={() => setSwitching(false)}>
          <div className="action-list book-list">
            {[
              {
                id: user.id,
                name: user.name,
                email: user.email,
                avatar: user.avatar,
              },
              ...books,
            ].map((p) => {
              const on = (viewing?.id ?? user.id) === p.id;
              return (
                <button
                  type="button"
                  key={p.id}
                  className={on ? "on" : ""}
                  onClick={() => {
                    setSwitching(false);
                    if (!on) setViewing(p.id === user.id ? null : p);
                  }}
                >
                  <Avatar person={p} />
                  <span>
                    {p.id === user.id ? "我的帳本" : `${p.name} 的帳本`}
                    <small>
                      {p.id === user.id
                        ? "可以記帳與修改"
                        : `${p.email} · 唯讀`}
                    </small>
                  </span>
                  {on && <Check size={17} />}
                </button>
              );
            })}
            <button
              type="button"
              onClick={() => {
                setSwitching(false);
                navigate("household");
              }}
            >
              <span className="avatar book-icon" aria-hidden="true">
                <HeartHandshake size={17} />
              </span>
              <span>
                家庭合併總覽
                <small>把多人加起來看總收入、總支出與淨資產</small>
              </span>
            </button>
          </div>
        </Sheet>
      )}
    </div>
  );
}

function TabLink({
  item,
  active,
}: {
  item: (typeof nav)[number];
  active: boolean;
}) {
  return (
    <a
      href={`#/${item.id}`}
      className={`tab ${active ? "on" : ""}`}
      aria-current={active ? "page" : undefined}
    >
      <span className="tab-icon">
        <item.icon size={22} />
      </span>
      <span>{item.label}</span>
    </a>
  );
}

function Notifications({ onClose }: { onClose: () => void }) {
  const list = useQuery({
    queryKey: ["notifications"],
    queryFn: () => api<Notification[]>("/notifications"),
  });
  return (
    <Sheet title="通知" side="right" size="sm" onClose={onClose}>
      {list.isPending ? (
        <p className="muted">讀取中…</p>
      ) : list.isError ? (
        <p className="form-error">讀取失敗，請稍後再試。</p>
      ) : !list.data.length ? (
        <Empty compact icon={Bell} title="沒有新通知">
          固定收支自動記帳時，會在這裡通知你。
        </Empty>
      ) : (
        <ul className="notice-list">
          {list.data.map((n) => (
            <li key={n.id}>
              <strong>{n.title}</strong>
              <p>{n.body}</p>
              <small>{relative(n.created_at)}</small>
            </li>
          ))}
        </ul>
      )}
    </Sheet>
  );
}

export function Logo({ size = 28 }: { size?: number }) {
  // 與 app/icon.svg 同一設計：M 的右半筆往上延伸成箭頭
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      aria-hidden="true"
      className="logo"
    >
      <defs>
        <linearGradient id="logo-bg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#3cc49c" />
          <stop offset="1" stopColor="#17684f" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="15" fill="url(#logo-bg)" />
      <g
        fill="none"
        stroke="#fff"
        strokeWidth="6.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M15 46V22l11 12.5 6.5-7.5 7 8L50 22" />
        <path d="M40.5 21.5H50.5V31.5" />
      </g>
    </svg>
  );
}
