"use client";
import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Crown,
  History,
  Search,
  ShieldCheck,
  ShieldOff,
  UserCheck,
  Users,
  X,
  BookOpen,
} from "lucide-react";
import { api, type Member } from "@/lib/api";
import { relative, stamp, stampDate } from "@/lib/format";
import { useAction, useApp } from "@/components/app-context";
import { Avatar } from "@/components/avatar";
import {
  Button,
  Card,
  Chip,
  Empty,
  ErrorLine,
  PageHead,
  Sheet,
  Skeleton,
  Switch,
} from "@/components/ui";

type AuditEvent = {
  id: string;
  action: "sync_approved" | "sync_revoked" | "admin_granted" | "admin_revoked";
  created_at: string;
  actor: string;
  actor_email: string;
  target: string | null;
  target_email: string | null;
};
type Filter = "all" | "admin" | "sync" | "pending";
/** 紀錄句型：{操作者} {前} {對象} {後} */
const actionText: Record<AuditEvent["action"], [string, string]> = {
  sync_approved: ["核准", "的同步資格"],
  sync_revoked: ["撤銷", "的同步資格"],
  admin_granted: ["將", "指定為管理員"],
  admin_revoked: ["移除", "的管理員權限"],
};

/** 成員管理：只有管理員看得到；只管權限，看不到任何人的財務資料 */
export function MembersPage() {
  const { viewing } = useApp();
  if (viewing) return <ReadOnlyNotice what="成員權限" />;
  return <Members />;
}
function Members() {
  const { user } = useApp();
  const members = useQuery({
    queryKey: ["members"],
    queryFn: () => api<Member[]>("/admin/users"),
    enabled: user.is_admin,
    refetchInterval: 15000,
    refetchOnWindowFocus: "always",
  });
  const audit = useQuery({
    queryKey: ["members", "audit"],
    queryFn: () => api<AuditEvent[]>("/admin/audit"),
    enabled: user.is_admin,
  });
  const { run, busy } = useAction();
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [showTest, setShowTest] = useState(false);
  const [confirm, setConfirm] = useState<{ m: Member; admin: boolean }>();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    // 即使 API 回傳相同資料，相對時間也必須繼續前進。
    const tick = () => setNow(Date.now());
    const timer = window.setInterval(tick, 15000);
    document.addEventListener("visibilitychange", tick);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  }, []);

  const all = members.data ?? [];
  // 更新期間舊版 API 尚無活動欄位時，保留原本登入時間的語意。
  const activityEnabled = all.some((m) => m.last_active_at !== undefined);
  const activityLabel = activityEnabled ? "最近活動" : "最近登入";
  const real = all.filter((m) => !m.test);
  const testCount = all.length - real.length;
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return (showTest ? all : real).filter((m) => {
      if (needle && !`${m.name} ${m.email}`.toLowerCase().includes(needle))
        return false;
      if (filter === "admin") return m.is_admin;
      if (filter === "sync") return m.is_admin || m.sync_approved;
      if (filter === "pending") return !m.is_admin && !m.sync_approved;
      return true;
    });
  }, [all, real, q, filter, showTest]);

  if (!user.is_admin)
    return (
      <Empty icon={ShieldOff} title="只有管理員可以管理成員">
        需要調整權限時，請聯絡管理員。
      </Empty>
    );

  const admins = real.filter((m) => m.is_admin).length;
  const syncing = real.filter((m) => m.is_admin || m.sync_approved).length;
  const pending = real.filter((m) => !m.is_admin && !m.sync_approved).length;

  return (
    <>
      <PageHead
        title="成員"
        subtitle="誰可以在這台主機同步銀行資料、誰可以管理成員。這裡看不到任何人的財務資料。"
      />
      {members.isError ? (
        <ErrorLine retry={() => members.refetch()}>成員清單讀取失敗</ErrorLine>
      ) : members.isPending ? (
        <Skeleton rows={4} h={64} />
      ) : (
        <div className="members">
          <div className="member-stats">
            <button
              type="button"
              className={filter === "all" ? "on" : ""}
              onClick={() => setFilter("all")}
            >
              <Users size={16} />
              <strong>{real.length}</strong>
              <span>成員</span>
            </button>
            <button
              type="button"
              className={filter === "admin" ? "on" : ""}
              onClick={() => setFilter("admin")}
            >
              <Crown size={16} />
              <strong>{admins}</strong>
              <span>管理員</span>
            </button>
            <button
              type="button"
              className={filter === "sync" ? "on" : ""}
              onClick={() => setFilter("sync")}
            >
              <UserCheck size={16} />
              <strong>{syncing}</strong>
              <span>可以同步</span>
            </button>
            <button
              type="button"
              className={`${filter === "pending" ? "on" : ""} ${pending ? "attention" : ""}`}
              onClick={() => setFilter("pending")}
            >
              <ShieldCheck size={16} />
              <strong>{pending}</strong>
              <span>等待核准</span>
            </button>
          </div>

          <section className="card flush">
            <header className="member-tools">
              <label className="search">
                <Search size={16} />
                <input
                  type="search"
                  placeholder="搜尋名稱或 Email"
                  aria-label="搜尋成員"
                  value={q}
                  onChange={(e) => setQ(e.target.value)}
                />
                {q && (
                  <button
                    type="button"
                    aria-label="清除搜尋"
                    onClick={() => setQ("")}
                  >
                    <X size={14} />
                  </button>
                )}
              </label>
              {testCount > 0 && (
                <Chip active={showTest} onClick={() => setShowTest(!showTest)}>
                  顯示測試帳號 {testCount}
                </Chip>
              )}
            </header>
            <div className="member-row member-head" aria-hidden="true">
              <span>成員</span>
              <span>{activityLabel}</span>
              <span>同步資格</span>
              <span>管理員</span>
            </div>
            {!shown.length ? (
              <p className="dim small empty-line">沒有符合的成員</p>
            ) : (
              <ul className="member-list">
                {shown.map((m) => {
                  const self = m.id === user.id;
                  const activity = activityEnabled
                    ? m.last_active_at
                    : m.last_login;
                  return (
                    <li
                      key={m.id}
                      className={`member-row ${m.test ? "is-test" : ""}`}
                    >
                      <span className="member-who">
                        <Avatar
                          person={m}
                          className={m.is_admin ? "admin" : ""}
                        />
                        <span className="member-main">
                          <strong>
                            {m.name}
                            {self && <em className="member-tag">你</em>}
                            {m.is_admin && (
                              <em className="member-tag admin">
                                <Crown size={11} /> 管理員
                              </em>
                            )}
                            {m.test && <em className="member-tag">測試</em>}
                          </strong>
                          <small>{m.email}</small>
                        </span>
                      </span>
                      <span className="member-cell" data-label={activityLabel}>
                        {activity ? (
                          <span title={stamp(activity)}>
                            {relative(activity, now)}
                          </span>
                        ) : (
                          <span className="dim">
                            {activityEnabled ? "尚無活動紀錄" : "尚無登入紀錄"}
                          </span>
                        )}
                        <small className="dim">
                          加入 {stampDate(m.created_at)}
                        </small>
                      </span>
                      <span className="member-cell" data-label="同步資格">
                        {m.is_admin ? (
                          <span className="dim small">管理員已包含</span>
                        ) : (
                          <label className="switch-line">
                            <span className="small">
                              {m.sync_approved ? "已核准" : "未核准"}
                            </span>
                            <Switch
                              checked={m.sync_approved}
                              label={`${m.name} 同步資格`}
                              disabled={busy}
                              onChange={(v) =>
                                run(`/admin/users/${m.id}/sync-approval`, {
                                  body: { approved: v },
                                  success: v
                                    ? `已核准 ${m.name}`
                                    : `已撤銷 ${m.name} 的同步資格`,
                                  toastError: true,
                                })
                              }
                            />
                          </label>
                        )}
                      </span>
                      <span className="member-cell" data-label="管理員">
                        {self ? (
                          <span className="dim small">不能移除自己</span>
                        ) : (
                          <Button
                            size="sm"
                            variant={m.is_admin ? "ghost" : "secondary"}
                            icon={m.is_admin ? ShieldOff : Crown}
                            disabled={busy || m.test}
                            onClick={() =>
                              setConfirm({ m, admin: !m.is_admin })
                            }
                          >
                            {m.is_admin ? "移除管理員" : "設為管理員"}
                          </Button>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
          </section>

          <Card
            title={
              <>
                <History size={17} /> 最近權限變更
              </>
            }
            pad={false}
          >
            {audit.isError ? (
              <p className="dim small empty-line">紀錄讀取失敗</p>
            ) : !audit.data?.length ? (
              <p className="dim small empty-line">還沒有權限變更紀錄</p>
            ) : (
              <ol className="audit-list">
                {audit.data.slice(0, 15).map((e) => (
                  <li key={e.id}>
                    <span className={`audit-dot ${e.action}`} />
                    <span>
                      <strong>{e.actor}</strong> {actionText[e.action][0]}{" "}
                      <strong>{e.target ?? "已刪除的成員"}</strong>{" "}
                      {actionText[e.action][1]}
                    </span>
                    <time dateTime={e.created_at} title={stamp(e.created_at)}>
                      {relative(e.created_at, now)}
                    </time>
                  </li>
                ))}
              </ol>
            )}
          </Card>
        </div>
      )}

      {confirm && (
        <Sheet
          title={confirm.admin ? "設為管理員" : "移除管理員"}
          size="sm"
          onClose={() => setConfirm(undefined)}
          busy={busy}
        >
          <div className="member-confirm">
            <Avatar person={confirm.m} size={56} className="lg" />
            <strong>{confirm.m.name}</strong>
            <small className="dim">{confirm.m.email}</small>
            <p>
              {confirm.admin
                ? "管理員可以核准或撤銷成員的同步資格，也可以指定、移除其他管理員。管理員看不到其他人的財務資料。成為管理員會同時取得同步資格。"
                : "移除後，這位成員不能再管理成員；同步資格會保留，需要的話可以另外關閉。"}
            </p>
            <div className="form-actions">
              <Button onClick={() => setConfirm(undefined)} disabled={busy}>
                取消
              </Button>
              <Button
                variant={confirm.admin ? "primary" : "danger"}
                busy={busy}
                onClick={async () => {
                  const ok = await run(`/admin/users/${confirm.m.id}/role`, {
                    body: { admin: confirm.admin },
                    success: confirm.admin
                      ? `${confirm.m.name} 已成為管理員`
                      : `已移除 ${confirm.m.name} 的管理員權限`,
                    toastError: true,
                  });
                  if (ok) setConfirm(undefined);
                }}
              >
                {confirm.admin ? "確定設為管理員" : "確定移除"}
              </Button>
            </div>
          </div>
        </Sheet>
      )}
    </>
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
