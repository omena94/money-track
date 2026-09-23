"use client";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Mail } from "lucide-react";
import { api } from "@/lib/api";
import { useAction, useApp } from "@/components/app-context";
import { Button, FormError } from "@/components/ui";
type GmailStatus = {
  connected: boolean;
  email?: string;
  auto_requested?: boolean;
  verified_at?: string;
  last_error?: string;
};
export function GmailVerification({
  connection,
  disabled,
}: {
  connection: string;
  disabled: boolean;
}) {
  const { user, viewing } = useApp();
  const action = useAction();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const status = useQuery({
    queryKey: ["gmail-verification", connection],
    queryFn: () => api<GmailStatus>(`/connections/${connection}/gmail`),
    enabled: !viewing && !disabled,
    refetchInterval: 5000,
  });
  if (viewing || disabled) return null;
  const link = async () => {
    setBusy(true);
    setError("");
    try {
      const result = await api<{ url: string }>(
        `/connections/${connection}/gmail/authorize`,
        {
          method: "POST",
          csrf: user.csrf_token,
          body: { auto_requested: true },
        },
      );
      window.location.assign(result.url);
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  const d = status.data;
  return (
    <section className="card detail-block gmail-card">
      <header>
        <strong>
          <Mail size={15} className="inline-icon" /> Gmail 自動讀取驗證碼
        </strong>
        {d?.connected && (
          <span className={`status-pill ${d.verified_at ? "good" : "info"}`}>
            {d.verified_at ? "自動取碼已驗收" : "待本人同步驗收"}
          </span>
        )}
      </header>
      {d?.connected ? (
        <dl className="gmail-facts">
          <div>
            <dt>已連結</dt>
            <dd>{d.email}</dd>
          </div>
          <div>
            <dt>排程</dt>
            <dd>
              {!d.verified_at
                ? "驗收成功後自動啟用"
                : d.auto_requested
                  ? "自動同步中"
                  : "手動（可在下方設定開啟）"}
            </dd>
          </div>
        </dl>
      ) : (
        <p className="hint-line">
          連結後，同步時會自動讀取富邦寄來的驗證信，不用每次手動輸入。
        </p>
      )}
      {d?.last_error && (
        <p className="hint-line warn-text" role="status">
          {d.last_error}
        </p>
      )}
      <div className="inline-actions">
        <Button size="sm" onClick={link} disabled={busy || action.busy}>
          {busy
            ? "前往 Google…"
            : d?.connected
              ? "重新連結／更換 Gmail"
              : "連結 Gmail"}
        </Button>
        {d?.connected && (
          <Button
            size="sm"
            variant="danger-ghost"
            disabled={busy || action.busy}
            onClick={() =>
              action.run(`/connections/${connection}/gmail`, {
                method: "DELETE",
                success: "Gmail 已解除連結，已恢復手動同步",
                toastError: true,
              })
            }
          >
            解除連結
          </Button>
        )}
      </div>
      <details className="gmail-more">
        <summary>權限與安全說明</summary>
        <ul>
          <li>
            每筆銀行連線可選擇自己的 Gmail，請選擇實際收到富邦驗證信的帳號。
          </li>
          <li>
            只搜尋富邦寄來、符合本次交易代碼及時間的驗證信，不寄信、不刪信。
          </li>
          <li>Google 的唯讀授權涵蓋信箱郵件。</li>
          <li>
            授權失效、取碼逾時或銀行驗證失敗時，會關閉排程並回到手動驗證。
          </li>
        </ul>
      </details>
      {(error || status.error) && (
        <FormError>{error || (status.error as Error).message}</FormError>
      )}
    </section>
  );
}
