"use client";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowDown, ChevronDown, Plus } from "lucide-react";
import Decimal from "decimal.js";
import { api, type Account } from "@/lib/api";
import { daysAgo, evaluate, money, symbol, today } from "@/lib/format";
import { useAccounts, useRecent } from "@/lib/queries";
import {
  readStore,
  useApp,
  writeStore,
  type ComposeOptions,
} from "@/components/app-context";
import {
  Button,
  CategoryIcon,
  Chip,
  Field,
  FormError,
  Segmented,
  Sheet,
} from "@/components/ui";
import { AccountForm } from "./account-form";
import { CategoryEditor } from "./category-manager";
import { useCategoryNames } from "@/lib/categories";

type Kind = "expense" | "income" | "transfer";

export function Composer({
  options,
  onClose,
}: {
  options: ComposeOptions;
  onClose: () => void;
}) {
  const accounts = useAccounts();
  const list = accounts.data ?? [];
  const [creating, setCreating] = useState(false);
  if (accounts.isPending)
    return (
      <Sheet title="記一筆" onClose={onClose}>
        <p className="muted">讀取帳戶中…</p>
      </Sheet>
    );
  if (!list.length || creating)
    return (
      <Sheet
        title={list.length ? "新增帳戶" : "先建立第一個帳戶"}
        description="每一筆收支都需要記在某個帳戶上，例如錢包、薪轉戶或信用卡。"
        onClose={onClose}
      >
        <AccountForm
          initialKind="cash"
          submitLabel={list.length ? "建立並回到記帳" : "建立帳戶，開始記帳"}
          onDone={() => setCreating(false)}
        />
      </Sheet>
    );
  return (
    <EntryForm
      accounts={list}
      options={options}
      onClose={onClose}
      onNewAccount={() => setCreating(true)}
    />
  );
}

function EntryForm({
  accounts,
  options,
  onClose,
  onNewAccount,
}: {
  accounts: Account[];
  options: ComposeOptions;
  onClose: () => void;
  onNewAccount: () => void;
}) {
  const { user, toast } = useApp();
  const client = useQueryClient();
  const recent = useRecent();
  const pick = (id?: string | null) =>
    accounts.find((a) => a.id === id)?.id ?? accounts[0].id;
  const [kind, setKind] = useState<Kind>(() => {
    const k = options.kind ?? readStore<Kind>("composer:kind", "expense");
    return k === "transfer" && accounts.length < 2 ? "expense" : k;
  });
  const names = useCategoryNames(kind);
  const [accountId, setAccountId] = useState(() =>
    pick(
      options.accountId ?? readStore<string | null>("composer:account", null),
    ),
  );
  const [toId, setToId] = useState(
    () => accounts.find((a) => a.id !== accountId)?.id ?? "",
  );
  const [amount, setAmount] = useState("");
  const [received, setReceived] = useState("");
  const [description, setDescription] = useState("");
  const [category, setCategory] = useState("");
  const [addingCategory, setAddingCategory] = useState(false);
  const [date, setDate] = useState(today());
  const [tags, setTags] = useState("");
  const [note, setNote] = useState("");
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const amountRef = useRef<HTMLInputElement>(null);
  const again = useRef(false);

  const account = accounts.find((a) => a.id === accountId)!;
  const target = accounts.find((a) => a.id === toId);
  const crossCurrency =
    kind === "transfer" && target && target.currency !== account.currency;
  const value = evaluate(amount);
  const isExpression = /\d[+-]/.test(amount.replace(/\s/g, ""));

  // 切換類型時，預設為這個類型最常用的分類
  useEffect(() => {
    if (kind === "transfer") return;
    const counts = new Map<string, number>();
    for (const e of recent.data ?? [])
      if (e.kind === kind)
        counts.set(e.category, (counts.get(e.category) ?? 0) + 1);
    const top = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
    setCategory(top ?? names.main[0] ?? "未分類");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);
  useEffect(() => {
    if (toId === accountId)
      setToId(accounts.find((a) => a.id !== accountId)?.id ?? "");
  }, [accountId, toId, accounts]);

  // 從最近紀錄學習：常用說明（附分類）與使用者自訂分類
  const history = useMemo(() => {
    const rows = (recent.data ?? []).filter((e) => e.kind === kind);
    const freq = new Map<
      string,
      { n: number; category: string; account: string }
    >();
    for (const e of rows) {
      if (e.split_parent_id) continue;
      const f = freq.get(e.description);
      if (f) f.n++;
      else
        freq.set(e.description, {
          n: 1,
          category: e.category,
          account: e.account_id,
        });
    }
    const descriptions = [...freq.entries()]
      .sort((a, b) => b[1].n - a[1].n)
      .map(([d, v]) => ({ description: d, ...v }));
    const used = [...new Set(rows.map((e) => e.category))];
    return { descriptions, used };
  }, [recent.data, kind]);
  const suggestions = useMemo(() => {
    const q = description.trim();
    return history.descriptions
      .filter((d) =>
        q ? d.description.includes(q) && d.description !== q : true,
      )
      .slice(0, 6);
  }, [history, description]);
  const categoryOptions = useMemo(() => {
    const base = names.main;
    const extra = history.used.filter(
      (c) => !base.includes(c) && !names.hidden.includes(c),
    );
    const all = [...base, ...extra];
    if (category && !all.includes(category)) all.push(category);
    return all;
  }, [names, history.used, category]);

  function chooseSuggestion(s: {
    description: string;
    category: string;
    account: string;
  }) {
    setDescription(s.description);
    setCategory(s.category);
    if (accounts.some((a) => a.id === s.account) && !options.accountId)
      setAccountId(s.account);
    amountRef.current?.focus();
  }

  async function submit(e?: FormEvent) {
    e?.preventDefault();
    setError("");
    if (value == null || new Decimal(value).lte(0)) {
      setError("請輸入大於 0 的金額");
      amountRef.current?.focus();
      return;
    }
    let receivedValue: string | null = null;
    if (crossCurrency) {
      receivedValue = evaluate(received);
      if (receivedValue == null || new Decimal(receivedValue).lte(0)) {
        setError(`跨幣別轉帳請填入實際轉入的 ${target!.currency} 金額`);
        return;
      }
    }
    const desc = description.trim() || (kind === "transfer" ? "帳戶轉帳" : "");
    if (!desc) {
      setError("請寫下這筆的說明，之後比較好找");
      return;
    }
    setBusy(true);
    try {
      if (kind === "transfer") {
        await api("/transfers", {
          method: "POST",
          csrf: user.csrf_token,
          body: {
            from: accountId,
            to: toId,
            amount: value,
            received_amount: receivedValue,
            occurred_on: date,
            description: desc,
          },
        });
      } else {
        const n = new Decimal(value).abs();
        await api("/transactions", {
          method: "POST",
          csrf: user.csrf_token,
          body: {
            account_id: accountId,
            kind,
            amount: (kind === "expense" ? n.negated() : n).toString(),
            description: desc,
            category: category || "未分類",
            occurred_on: date,
            tags: tags
              .split(/[,，]/)
              .map((t) => t.trim())
              .filter(Boolean),
            note: note.trim(),
          },
        });
      }
      writeStore("composer:kind", kind);
      writeStore("composer:account", accountId);
      await client.invalidateQueries();
      toast(
        kind === "transfer"
          ? `已轉帳 ${money(value, account.currency)}`
          : `已記下「${desc}」${money(kind === "expense" ? `-${value}` : value, account.currency)}`,
      );
      if (again.current) {
        setAmount("");
        setReceived("");
        setDescription("");
        setTags("");
        setNote("");
        amountRef.current?.focus();
      } else onClose();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
      again.current = false;
    }
  }

  return (
    <Sheet
      title="記一筆"
      onClose={onClose}
      busy={busy}
      footer={
        <>
          <Button
            variant="ghost"
            disabled={busy}
            onClick={() => {
              again.current = true;
              void submit();
            }}
            title="儲存後保留帳戶與日期，繼續輸入下一筆"
          >
            儲存並再記一筆
          </Button>
          <Button
            type="submit"
            form="composer"
            variant="primary"
            busy={busy}
            className="grow"
          >
            儲存
          </Button>
        </>
      }
    >
      <form
        id="composer"
        className="form composer"
        onSubmit={submit}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
        }}
      >
        <Segmented<Kind>
          label="類型"
          value={kind}
          onChange={setKind}
          options={[
            { value: "expense", label: "支出" },
            { value: "income", label: "收入" },
            {
              value: "transfer",
              label: "轉帳",
              disabled: accounts.length < 2,
            },
          ]}
        />
        <FormError>{error}</FormError>
        <label className={`amount-input kind-${kind}`}>
          <span className="sr-only">金額</span>
          <span className="amount-prefix">
            {kind === "expense" ? "−" : kind === "income" ? "+" : ""}
            {symbol(account.currency)}
          </span>
          <input
            ref={amountRef}
            name="amount"
            data-autofocus
            inputMode="decimal"
            autoComplete="off"
            placeholder="0"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            aria-describedby="amount-help"
            style={{ width: `calc(${Math.max(1, amount.length)}ch + 8px)` }}
          />
        </label>
        <p id="amount-help" className="amount-help">
          {isExpression && value
            ? `＝ ${money(value, account.currency)}`
            : "可輸入算式，例如 120+45"}
        </p>

        {kind === "transfer" ? (
          <div className="transfer-pair">
            <Field label="從">
              <AccountPicker
                accounts={accounts}
                value={accountId}
                onChange={setAccountId}
              />
            </Field>
            <span className="transfer-arrow" aria-hidden="true">
              <ArrowDown size={16} />
            </span>
            <Field label="到">
              <AccountPicker
                accounts={accounts.filter((a) => a.id !== accountId)}
                value={toId}
                onChange={setToId}
              />
            </Field>
            {crossCurrency && (
              <Field
                label={`實際轉入（${target!.currency}）`}
                hint="不同幣別，請填銀行實際入帳的金額"
              >
                <div className="affix">
                  <span>{symbol(target!.currency)}</span>
                  <input
                    name="received_amount"
                    inputMode="decimal"
                    autoComplete="off"
                    value={received}
                    onChange={(e) => setReceived(e.target.value)}
                  />
                </div>
              </Field>
            )}
            <Field label="說明" optional>
              <input
                name="description"
                maxLength={500}
                placeholder="帳戶轉帳"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </Field>
            <p className="hint-line">
              繳信用卡費請用轉帳：從銀行帳戶轉到信用卡，不會重複算成支出。
            </p>
          </div>
        ) : (
          <>
            <Field label="說明">
              <input
                name="description"
                maxLength={500}
                autoComplete="off"
                placeholder={
                  kind === "expense" ? "花在哪裡？例如：午餐" : "例如：九月薪資"
                }
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </Field>
            {suggestions.length > 0 && (
              <div className="suggest" aria-label="常用說明">
                {suggestions.map((s) => (
                  <button
                    type="button"
                    key={s.description}
                    onClick={() => chooseSuggestion(s)}
                  >
                    {s.description}
                    <small>{s.category}</small>
                  </button>
                ))}
              </div>
            )}
            <fieldset className="field">
              <legend className="field-label">分類</legend>
              <div className="category-grid">
                {categoryOptions.map((c) => (
                  <button
                    type="button"
                    key={c}
                    className={`category-option ${category === c ? "on" : ""}`}
                    aria-pressed={category === c}
                    onClick={() => setCategory(c)}
                  >
                    <CategoryIcon category={c} size={34} />
                    <span>{c}</span>
                  </button>
                ))}
                <button
                  type="button"
                  className="category-option"
                  onClick={() => setAddingCategory(true)}
                >
                  <span
                    className="cat-icon add"
                    style={{ width: 34, height: 34 }}
                  >
                    <Plus size={16} />
                  </span>
                  <span>新增</span>
                </button>
              </div>
              {addingCategory && (
                <CategoryEditor
                  kind={kind === "income" ? "income" : "expense"}
                  onClose={() => setAddingCategory(false)}
                  onSaved={setCategory}
                />
              )}
            </fieldset>
            <Field label="帳戶">
              <AccountPicker
                accounts={accounts}
                value={accountId}
                onChange={setAccountId}
                onNew={onNewAccount}
              />
            </Field>
          </>
        )}

        <fieldset className="field">
          <legend className="field-label">日期</legend>
          <div className="date-row">
            <Chip active={date === today()} onClick={() => setDate(today())}>
              今天
            </Chip>
            <Chip
              active={date === daysAgo(1)}
              onClick={() => setDate(daysAgo(1))}
            >
              昨天
            </Chip>
            <input
              type="date"
              name="occurred_on"
              aria-label="選擇日期"
              value={date}
              max={today()}
              required
              onChange={(e) => setDate(e.target.value)}
            />
          </div>
        </fieldset>

        {kind !== "transfer" && (
          <div className="more">
            <button
              type="button"
              className="more-toggle"
              aria-expanded={more || !!tags || !!note}
              onClick={() => setMore(!more)}
            >
              標籤與備註
              <ChevronDown size={16} />
            </button>
            {(more || tags || note) && (
              <div className="form">
                <Field label="標籤" hint="用逗號分隔，例如：出差, 家庭">
                  <input
                    name="tags"
                    value={tags}
                    onChange={(e) => setTags(e.target.value)}
                  />
                </Field>
                <Field label="備註">
                  <textarea
                    name="note"
                    maxLength={2000}
                    rows={2}
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                  />
                </Field>
              </div>
            )}
          </div>
        )}
        <p className="kbd-hint">
          <kbd>Ctrl</kbd> + <kbd>Enter</kbd> 儲存 · <kbd>Esc</kbd> 關閉
        </p>
      </form>
    </Sheet>
  );
}

export function AccountPicker({
  accounts,
  value,
  onChange,
  onNew,
}: {
  accounts: Account[];
  value: string;
  onChange: (id: string) => void;
  onNew?: () => void;
}) {
  if (accounts.length > 6)
    return (
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {accounts.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}（{a.currency}）
          </option>
        ))}
      </select>
    );
  return (
    <div className="pick-row" role="radiogroup">
      {accounts.map((a) => (
        <button
          type="button"
          role="radio"
          aria-checked={a.id === value}
          key={a.id}
          className={`pick ${a.id === value ? "on" : ""}`}
          onClick={() => onChange(a.id)}
        >
          <span>{a.name}</span>
          {a.currency !== "TWD" && <small>{a.currency}</small>}
        </button>
      ))}
      {onNew && (
        <button type="button" className="pick add" onClick={onNew}>
          <Plus size={14} />
          新帳戶
        </button>
      )}
    </div>
  );
}
