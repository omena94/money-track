"use client";
import { useState, type FormEvent } from "react";
import {
  Banknote,
  Building2,
  CreditCard,
  HandCoins,
  House,
  Landmark,
  Shield,
  TrendingUp,
  Wallet,
  type LucideIcon,
} from "lucide-react";
import Decimal from "decimal.js";
import { api, type Account } from "@/lib/api";
import {
  currencies,
  evaluate,
  kinds,
  liabilityKinds,
  symbol,
} from "@/lib/format";
import { useApp } from "@/components/app-context";
import { Button, Field, FormError } from "@/components/ui";
import { useQueryClient } from "@tanstack/react-query";

export const kindIcons: Record<string, LucideIcon> = {
  cash: Banknote,
  bank: Landmark,
  credit: CreditCard,
  investment: TrendingUp,
  loan: HandCoins,
  property: House,
  insurance: Shield,
  other: Wallet,
};
/** 手動帳戶類型的圖示主色（非銀行、無官方圖示時使用） */
export const kindTones: Record<string, string> = {
  cash: "var(--c5)",
  bank: "var(--c2)",
  credit: "var(--c4)",
  investment: "var(--c6)",
  loan: "var(--c1)",
  property: "var(--c7)",
  insurance: "var(--c3)",
  other: "var(--c8)",
};
export { Building2 };

/** 新增帳戶。負債類型改問「目前欠款」，自動轉成負數，使用者不必自己想正負號 */
export function AccountForm({
  initialKind = "bank",
  onDone,
  submitLabel = "建立帳戶",
}: {
  initialKind?: string;
  onDone: (account: Account) => void;
  submitLabel?: string;
}) {
  const { user, toast } = useApp();
  const client = useQueryClient();
  const [kind, setKind] = useState(initialKind);
  const [currency, setCurrency] = useState("TWD");
  const [balance, setBalance] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const liability = liabilityKinds.has(kind);
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const v = Object.fromEntries(new FormData(e.currentTarget)) as Record<
      string,
      string
    >;
    const value = balance.trim() === "" ? "0" : evaluate(balance);
    if (value == null) {
      setError("金額格式不正確，請輸入數字");
      return;
    }
    const n = new Decimal(value);
    const opening = liability ? n.abs().negated() : n;
    setBusy(true);
    setError("");
    try {
      const account = await api<Account>("/accounts", {
        method: "POST",
        csrf: user.csrf_token,
        body: {
          name: v.name.trim(),
          kind,
          currency,
          opening_balance: opening.toString(),
          institution: v.institution?.trim() || null,
        },
      });
      await client.invalidateQueries();
      toast(`已建立「${v.name.trim()}」`);
      onDone(account);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form onSubmit={submit} className="form" id="account-form">
      <FormError>{error}</FormError>
      <fieldset className="field">
        <legend className="field-label">類型</legend>
        <div className="kind-grid">
          {Object.entries(kinds).map(([k, label]) => {
            const Icon = kindIcons[k];
            return (
              <button
                type="button"
                key={k}
                className={`kind-option ${kind === k ? "on" : ""}`}
                aria-pressed={kind === k}
                onClick={() => setKind(k)}
              >
                <Icon size={20} />
                <span>{label}</span>
              </button>
            );
          })}
        </div>
      </fieldset>
      <Field label="名稱">
        <input
          name="name"
          required
          maxLength={120}
          data-autofocus
          placeholder={
            kind === "credit"
              ? "例如：玉山 Pi 卡"
              : kind === "cash"
                ? "例如：錢包"
                : kind === "investment"
                  ? "例如：元大證券"
                  : "例如：薪轉帳戶"
          }
        />
      </Field>
      <div className="grid-2">
        <Field
          label={
            liability
              ? "目前欠款"
              : kind === "investment"
                ? "帳戶現金"
                : "目前餘額"
          }
          hint={
            liability
              ? "填正數即可，會自動記為負債"
              : kind === "investment"
                ? "只填交割現金，持股稍後在投資頁加入"
                : undefined
          }
        >
          <div className="affix">
            <span>{symbol(currency)}</span>
            <input
              name="opening_balance"
              inputMode="decimal"
              autoComplete="off"
              placeholder="0"
              value={balance}
              onChange={(e) => setBalance(e.target.value)}
            />
          </div>
        </Field>
        <Field label="幣別">
          <select
            name="currency"
            value={currency}
            onChange={(e) => setCurrency(e.target.value)}
          >
            {currencies.map((c) => (
              <option key={c}>{c}</option>
            ))}
          </select>
        </Field>
      </div>
      <Field label="金融機構" optional>
        <input
          name="institution"
          maxLength={120}
          placeholder="銀行、券商或發卡機構"
        />
      </Field>
      <Button
        type="submit"
        variant="primary"
        size="lg"
        busy={busy}
        className="block"
      >
        {submitLabel}
      </Button>
    </form>
  );
}
