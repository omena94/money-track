"use client";
import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Plus, WandSparkles } from "lucide-react";
import { api, type Entry } from "@/lib/api";
import { UNCATEGORIZED, useCategoryNames } from "@/lib/categories";
import { useAction, useApp } from "@/components/app-context";
import { Button, CategoryIcon, Field, FormError } from "@/components/ui";
import { CategoriesSheet, CategoryEditor } from "./category-manager";

/** 從交易說明猜一個適合當規則的關鍵字：去掉遮罩帳號、代碼，取最後一個有意義的詞 */
export function suggestKeyword(description: string) {
  const words = description
    .split(/[\s　]+/)
    .map((w) => w.trim())
    .filter(
      (w) =>
        w &&
        !w.includes("*") &&
        !w.startsWith("_") &&
        !/^[\dA-Z-]{4,}$/i.test(w.normalize("NFKC")) &&
        (w.match(/\d/g)?.length ?? 0) < 3,
    );
  return words.at(-1) ?? description.trim().slice(0, 20);
}

/** 分類選擇：我的分類一鍵點選；不算收支的分類另列；沒有合適的可以直接新增 */
export function CategoryPicker({
  value,
  onChange,
  kind,
}: {
  value: string;
  onChange: (v: string) => void;
  kind: string;
}) {
  const { main, flow } = useCategoryNames(kind);
  const [adding, setAdding] = useState(false);
  const known = [...main, ...flow, UNCATEGORIZED];
  // 目前值不在清單（例如隱藏的分類）時也要顯示出來
  const extra = value && !known.includes(value) ? [value] : [];
  const item = (c: string) => (
    <button
      type="button"
      role="radio"
      aria-checked={value === c}
      key={c}
      className={value === c ? "on" : ""}
      onClick={() => onChange(c)}
    >
      <CategoryIcon category={c} size={30} />
      <span>{c}</span>
    </button>
  );
  return (
    <div className="cat-picker">
      <div className="cat-grid" role="radiogroup" aria-label="分類">
        {[...main, ...extra].map(item)}
        <button
          type="button"
          className="cat-add"
          onClick={() => setAdding(true)}
        >
          <span className="cat-icon add" style={{ width: 30, height: 30 }}>
            <Plus size={15} />
          </span>
          <span>新增</span>
        </button>
      </div>
      <div className="cat-grid-sub">
        <span className="dim small">不算收支</span>
        <div className="cat-grid" role="radiogroup" aria-label="不算收支的分類">
          {[...flow, UNCATEGORIZED].map(item)}
        </div>
      </div>
      {adding && (
        <CategoryEditor
          kind={kind === "income" ? "income" : "expense"}
          onClose={() => setAdding(false)}
          onSaved={onChange}
        />
      )}
    </div>
  );
}

/** 同步交易：修改分類、標籤、備註，並可順便建立規則 */
export function SourceEntryForm({
  entry,
  onCancel,
  onDone,
}: {
  entry: Entry;
  onCancel: () => void;
  onDone: () => void;
}) {
  const { toast } = useApp();
  const { run, busy, error, setError } = useAction();
  const [category, setCategory] = useState(
    entry.category === "未分類" ? "" : entry.category,
  );
  const [remember, setRemember] = useState(entry.category_source !== "user");
  const [keyword, setKeyword] = useState(() =>
    suggestKeyword(entry.description),
  );
  const [tags, setTags] = useState(
    entry.tags.filter((t) => t !== "自動同步").join(", "),
  );
  const [note, setNote] = useState(entry.note ?? "");
  async function submit(e: FormEvent) {
    e.preventDefault();
    const c = category.trim();
    if (!c) return setError("請選擇分類");
    if (remember && keyword.replace(/\s/g, "").length < 2)
      return setError("關鍵字至少 2 個字");
    const ok = await run(`/source-transactions/${entry.id}`, {
      method: "PATCH",
      body: {
        category: c,
        note,
        tags: tags
          .split(/[,，]/)
          .map((t) => t.trim())
          .filter(Boolean),
        rule: remember ? keyword : undefined,
      },
    });
    if (!ok) return;
    toast(
      remember
        ? `已分類為「${c}」並儲存規則；其他舊交易可至分類規則選擇期間重新分類`
        : `已分類為「${c}」`,
    );
    onDone();
  }
  return (
    <form className="form" onSubmit={submit}>
      <FormError>{error}</FormError>
      <Field label="分類">
        <CategoryPicker
          value={category}
          onChange={(v) => {
            setCategory(v);
            setError("");
          }}
          kind={entry.kind}
        />
      </Field>
      <label className="rule-toggle">
        <input
          type="checkbox"
          checked={remember}
          onChange={(e) => setRemember(e.target.checked)}
        />
        <span>
          <strong>以後自動分類</strong>
          <small>說明含有這個關鍵字的同步交易，都會自動歸到這個分類</small>
        </span>
      </label>
      {remember && (
        <Field label="關鍵字" hint="比對時不分大小寫、忽略空白">
          <input
            value={keyword}
            maxLength={80}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </Field>
      )}
      <Field label="標籤" hint="用逗號分隔">
        <input value={tags} onChange={(e) => setTags(e.target.value)} />
      </Field>
      <Field label="備註">
        <textarea
          rows={3}
          maxLength={2000}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </Field>
      <div className="form-actions">
        <Button onClick={onCancel} disabled={busy}>
          取消
        </Button>
        <Button type="submit" variant="primary" busy={busy}>
          儲存
        </Button>
      </div>
    </form>
  );
}

/** 分類規則（舊入口）：開啟分類面板的規則頁 */
export function RulesSheet({ onClose }: { onClose: () => void }) {
  return <CategoriesSheet tab="rules" onClose={onClose} />;
}

/** 依目前規則重新分類指定期間的同步交易 */
export function ReclassifyForm({ disabled }: { disabled: boolean }) {
  const { user, viewing } = useApp();
  const client = useQueryClient();
  const [scope, setScope] = useState("all");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState("");
  async function submit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setResult("");
    if (viewing) return setError("請先切回自己的帳本");
    if (scope !== "all" && (!start || !end || start > end))
      return setError("請填寫有效期間，開始需早於或等於結束");
    setBusy(true);
    const range = scope === "all" ? "全部期間" : `${start} 至 ${end}`;
    try {
      const data = await api<{ processed: number; protected: number }>(
        "/category-rules/reclassify",
        {
          method: "POST",
          csrf: user.csrf_token,
          body: scope === "all" ? { scope } : { scope, start, end },
        },
      );
      setResult(
        `${range}：已重新分類 ${data.processed} 筆，保留 ${data.protected} 筆手動分類。`,
      );
      await client.invalidateQueries();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="form reclassify-form" onSubmit={submit}>
      <h3>重新分類同步交易</h3>
      <p className="muted small">
        依目前規則重算選定期間（包含起訖日），保留手動分類、標籤與備註。未命中任何規則的交易會回到未分類。
      </p>
      <Field label="套用期間">
        <select
          aria-label="套用期間"
          value={scope}
          disabled={busy}
          onChange={(e) => {
            setScope(e.target.value);
            setStart("");
            setEnd("");
            setResult("");
            setError("");
          }}
        >
          <option value="all">全部交易</option>
          <option value="months">月份區間</option>
          <option value="dates">日期區間</option>
        </select>
      </Field>
      {scope !== "all" && (
        <div className="reclassify-dates">
          <Field label={scope === "months" ? "開始月份" : "開始日期"}>
            <input
              aria-label="開始期間"
              type={scope === "months" ? "month" : "date"}
              value={start}
              max={end || undefined}
              required
              disabled={busy}
              onChange={(e) => {
                setStart(e.target.value);
                setResult("");
              }}
            />
          </Field>
          <Field label={scope === "months" ? "結束月份" : "結束日期"}>
            <input
              aria-label="結束期間"
              type={scope === "months" ? "month" : "date"}
              value={end}
              min={start || undefined}
              required
              disabled={busy}
              onChange={(e) => {
                setEnd(e.target.value);
                setResult("");
              }}
            />
          </Field>
        </div>
      )}
      <FormError>{error}</FormError>
      {result && (
        <p role="status" className="small">
          {result}
        </p>
      )}
      <Button
        type="submit"
        icon={WandSparkles}
        busy={busy}
        disabled={disabled || !!viewing}
      >
        依目前規則重新分類
      </Button>
    </form>
  );
}
