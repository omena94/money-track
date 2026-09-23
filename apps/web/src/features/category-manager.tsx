"use client";
import { useMemo, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  EyeOff,
  Plus,
  Search,
  Sparkles,
  Trash2,
  WandSparkles,
  X,
} from "lucide-react";
import { api, type CategoryRule, type DefaultCategoryRule } from "@/lib/api";
import {
  UNCATEGORIZED,
  categoryColors,
  categoryIconSet,
  colorVar,
  useCategories,
  type UserCategory,
} from "@/lib/categories";
import { useAction, useApp } from "@/components/app-context";
import {
  Button,
  CategoryIcon,
  Empty,
  ErrorLine,
  Field,
  FormError,
  Segmented,
  Sheet,
  Skeleton,
  Switch,
} from "@/components/ui";
import { ReclassifyForm } from "./categorize";

type Tab = "categories" | "rules";
type KindTab = "expense" | "income" | "flow";
const kindLabels: Record<KindTab, string> = {
  expense: "支出",
  income: "收入",
  flow: "不算收支",
};
const kindHints: Record<KindTab, string> = {
  expense: "錢花出去的去向。點一個分類可以換圖示、顏色或改名。",
  income: "錢從哪裡來。薪資、獎金之外，也可以新增房租收入、接案等。",
  flow: "錢在自己的帳戶之間移動（轉帳、繳卡費、買賣投資），不算收入也不算支出，分析時會排除。",
};

/** 分類與自動分類規則：收支頁、分析頁都從這裡管理 */
export function CategoriesSheet({
  tab: initial = "categories",
  onClose,
}: {
  tab?: Tab;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<Tab>(initial);
  return (
    <Sheet
      title="分類"
      description="自己決定有哪些收入與支出分類，以及同步交易要怎麼自動分類"
      side="right"
      size="lg"
      onClose={onClose}
    >
      <Segmented
        label="分類設定"
        value={tab}
        onChange={setTab}
        options={[
          { value: "categories", label: "我的分類" },
          { value: "rules", label: "自動分類規則" },
        ]}
      />
      <div className="catman">
        {tab === "categories" ? <CategoriesTab /> : <RulesTab />}
      </div>
    </Sheet>
  );
}

/* ---------------------------------------------------------------------------
   我的分類
   ------------------------------------------------------------------------ */
function CategoriesTab() {
  const all = useCategories();
  const { run, busy } = useAction();
  const [kind, setKind] = useState<KindTab>("expense");
  const [editing, setEditing] = useState<UserCategory | "new" | null>(null);
  const list = all.filter((c) => c.kind === kind);
  const loaded = all.some((c) => c.id !== c.name);

  const move = (i: number, d: -1 | 1) => {
    const ids = list.map((c) => c.id);
    [ids[i], ids[i + d]] = [ids[i + d], ids[i]];
    run("/categories/order", {
      method: "PUT",
      body: { ids },
      toastError: true,
    });
  };

  return (
    <>
      <Segmented
        label="分類類型"
        size="sm"
        value={kind}
        onChange={setKind}
        options={(Object.keys(kindLabels) as KindTab[]).map((k) => ({
          value: k,
          label: `${kindLabels[k]} ${all.filter((c) => c.kind === k).length}`,
        }))}
      />
      <p className="dim small catman-hint">{kindHints[kind]}</p>
      <ul className="catman-list">
        {list.map((c, i) => (
          <li key={c.id} className={c.hidden ? "is-hidden" : ""}>
            <button
              type="button"
              className="catman-row"
              disabled={!loaded}
              onClick={() => setEditing(c)}
            >
              <CategoryIcon category={c.name} size={36} />
              <span className="catman-main">
                <strong>
                  {c.name}
                  {c.hidden && (
                    <span className="catman-badge">
                      <EyeOff size={11} /> 已隱藏
                    </span>
                  )}
                </strong>
                <small>
                  {[
                    c.builtin ? "內建" : "自訂",
                    c.keywords > 0 && `${c.keywords} 個內建關鍵字`,
                    c.rules > 0 && `${c.rules} 條我的規則`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </small>
              </span>
              <ChevronRight size={16} className="row-chevron" />
            </button>
            <span className="catman-order">
              <button
                type="button"
                aria-label={`把${c.name}往上移`}
                disabled={busy || !loaded || i === 0}
                onClick={() => move(i, -1)}
              >
                <ArrowUp size={14} />
              </button>
              <button
                type="button"
                aria-label={`把${c.name}往下移`}
                disabled={busy || !loaded || i === list.length - 1}
                onClick={() => move(i, 1)}
              >
                <ArrowDown size={14} />
              </button>
            </span>
          </li>
        ))}
      </ul>
      <Button icon={Plus} onClick={() => setEditing("new")} disabled={!loaded}>
        新增{kindLabels[kind]}分類
      </Button>
      <p className="dim small catman-hint">
        「{UNCATEGORIZED}」是系統分類，放還沒分好的交易，不能修改。
      </p>
      {editing && (
        <CategoryEditor
          category={editing === "new" ? undefined : editing}
          kind={kind}
          onClose={() => setEditing(null)}
        />
      )}
    </>
  );
}

/** 新增或編輯分類：名稱、圖示、顏色、隱藏、刪除 */
export function CategoryEditor({
  category,
  kind: initialKind = "expense",
  onClose,
  onSaved,
}: {
  category?: UserCategory;
  kind?: KindTab;
  onClose: () => void;
  onSaved?: (name: string) => void;
}) {
  const all = useCategories();
  const { run, busy, error, setError } = useAction();
  const [name, setName] = useState(category?.name ?? "");
  const [kind, setKind] = useState<KindTab>(category?.kind ?? initialKind);
  const [icon, setIcon] = useState(category?.icon ?? "tag");
  const [color, setColor] = useState(
    category?.color ?? categoryColors[all.filter((c) => !c.hidden).length % 8],
  );
  const [hidden, setHidden] = useState(category?.hidden ?? false);
  const [moveTo, setMoveTo] = useState(UNCATEGORIZED);
  const [confirm, setConfirm] = useState(false);
  const Icon = categoryIconSet.find(([k]) => k === icon)?.[1];
  const targets = all.filter(
    (c) => c.id !== category?.id && c.kind === (category?.kind ?? kind),
  );

  async function save(e: FormEvent) {
    e.preventDefault();
    const n = name.trim();
    if (!n) return setError("請輸入分類名稱");
    if (n === UNCATEGORIZED) return setError("「未分類」是系統保留名稱");
    const ok = category
      ? await run(`/categories/${category.id}`, {
          method: "PATCH",
          body: {
            ...(category.builtin ? {} : { name: n }),
            icon,
            color,
            hidden,
          },
          success:
            !category.builtin && n !== category.name
              ? `已改名為「${n}」，過去的交易、規則與預算也一起更新`
              : "分類已更新",
        })
      : await run("/categories", {
          body: { name: n, kind, icon, color },
          success: `已新增「${n}」`,
        });
    if (ok) {
      onSaved?.(n);
      onClose();
    }
  }
  async function remove() {
    if (!category) return;
    const ok = await run(
      `/categories/${category.id}?move_to=${encodeURIComponent(moveTo)}`,
      {
        method: "DELETE",
        success: `已刪除「${category.name}」，原本的交易移到「${moveTo}」`,
      },
    );
    if (ok) onClose();
  }

  return (
    <Sheet
      title={category ? "編輯分類" : "新增分類"}
      onClose={onClose}
      busy={busy}
    >
      <form className="form" onSubmit={save}>
        <div className="catman-preview">
          <span
            className="cat-icon"
            style={{
              width: 56,
              height: 56,
              ["--cat" as string]: colorVar(color),
            }}
          >
            {Icon && <Icon size={26} />}
          </span>
          <strong>{name.trim() || "分類名稱"}</strong>
        </div>
        <FormError>{error}</FormError>
        <Field
          label="名稱"
          hint={
            category?.builtin
              ? "內建分類供自動分類關鍵字使用，不能改名"
              : category
                ? "改名後，過去的交易、規則與預算會一起更新"
                : undefined
          }
        >
          <input
            value={name}
            maxLength={30}
            autoFocus={!category}
            disabled={category?.builtin}
            placeholder="例如：寵物、房租、咖啡"
            onChange={(e) => {
              setName(e.target.value);
              setError("");
            }}
          />
        </Field>
        {!category && (
          <Field label="類型">
            <Segmented
              label="類型"
              value={kind}
              onChange={setKind}
              options={[
                { value: "expense", label: "支出" },
                { value: "income", label: "收入" },
                { value: "flow", label: "不算收支" },
              ]}
            />
            {kind === "flow" && (
              <small className="field-hint">
                這個分類的交易視為自己帳戶間的資金移動，不計入收入、支出與分析。
              </small>
            )}
          </Field>
        )}
        <Field label="圖示">
          <div className="icon-grid" role="radiogroup" aria-label="圖示">
            {categoryIconSet.map(([key, I]) => (
              <button
                type="button"
                role="radio"
                key={key}
                aria-checked={icon === key}
                aria-label={key}
                className={icon === key ? "on" : ""}
                style={{ ["--cat" as string]: colorVar(color) }}
                onClick={() => setIcon(key)}
              >
                <I size={18} />
              </button>
            ))}
          </div>
        </Field>
        <Field label="顏色">
          <div className="color-row" role="radiogroup" aria-label="顏色">
            {categoryColors.map((c) => (
              <button
                type="button"
                role="radio"
                key={c}
                aria-checked={color === c}
                aria-label={`顏色 ${c}`}
                className={color === c ? "on" : ""}
                style={{ background: colorVar(c) }}
                onClick={() => setColor(c)}
              />
            ))}
          </div>
        </Field>
        {category && (
          <div className="catman-switch">
            <span>
              <strong>在選單中隱藏</strong>
              <small>
                用不到的分類可以藏起來；已經用過的交易仍會保留這個分類
              </small>
            </span>
            <Switch
              checked={hidden}
              onChange={setHidden}
              label="在選單中隱藏"
            />
          </div>
        )}
        <div className="form-actions">
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {category ? "儲存" : "新增"}
          </Button>
        </div>
      </form>
      {category && !category.builtin && (
        <section className="catman-danger">
          <h3>刪除分類</h3>
          <p className="dim small">
            原本屬於「{category.name}」的交易、預算與規則要移到哪裡？
          </p>
          <div className="catman-danger-row">
            <select
              aria-label="移到的分類"
              value={moveTo}
              onChange={(e) => {
                setMoveTo(e.target.value);
                setConfirm(false);
              }}
            >
              <option value={UNCATEGORIZED}>
                {UNCATEGORIZED}（規則與預算一併刪除）
              </option>
              {targets.map((c) => (
                <option key={c.id} value={c.name}>
                  {c.name}
                </option>
              ))}
            </select>
            <Button
              variant={confirm ? "danger" : "secondary"}
              icon={Trash2}
              busy={busy && confirm}
              onClick={() => (confirm ? remove() : setConfirm(true))}
            >
              {confirm ? "確定刪除" : "刪除"}
            </Button>
          </div>
        </section>
      )}
    </Sheet>
  );
}

/* ---------------------------------------------------------------------------
   自動分類規則：依分類分組、關鍵字用標籤呈現，一眼看出哪些字會歸到哪裡
   ------------------------------------------------------------------------ */
function RulesTab() {
  const all = useCategories();
  const { user, viewing } = useApp();
  const client = useQueryClient();
  const rules = useQuery({
    queryKey: ["category-rules"],
    queryFn: () => api<CategoryRule[]>("/category-rules"),
  });
  const defaults = useQuery({
    queryKey: ["category-rules", "defaults"],
    queryFn: () => api<DefaultCategoryRule[]>("/category-rules/defaults"),
  });
  const { run, busy, error, setError } = useAction();
  const [pattern, setPattern] = useState("");
  const [q, setQ] = useState("");
  const [added, setAdded] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState("");

  const order = useMemo(() => new Map(all.map((c, i) => [c.name, i])), [all]);
  const needle = q.trim().toLowerCase();
  const group = <T extends { category: string; pattern: string }>(
    list: T[],
  ) => {
    const map = new Map<string, T[]>();
    for (const r of list) {
      if (
        needle &&
        !`${r.category} ${r.pattern}`.toLowerCase().includes(needle)
      )
        continue;
      map.set(r.category, [...(map.get(r.category) ?? []), r]);
    }
    return [...map].sort(
      (a, b) => (order.get(a[0]) ?? 999) - (order.get(b[0]) ?? 999),
    );
  };
  const mine = useMemo(
    () => group(rules.data ?? []),
    [rules.data, needle, order],
  ); // eslint-disable-line react-hooks/exhaustive-deps
  const builtin = useMemo(
    () => group(defaults.data ?? []),
    [defaults.data, needle, order],
  ); // eslint-disable-line react-hooks/exhaustive-deps

  async function add(category: string) {
    const p = pattern.trim();
    if (p.replace(/\s/g, "").length < 2) return setError("關鍵字至少 2 個字");
    if (
      await run("/category-rules", {
        body: { pattern: p, category },
        success: `「${p}」之後會自動歸到${category}`,
      })
    ) {
      setPattern("");
      setAdded(category);
      setApplied("");
    }
  }
  async function applyAll() {
    if (viewing) return;
    setApplying(true);
    try {
      const r = await api<{ processed: number; protected: number }>(
        "/category-rules/reclassify",
        { method: "POST", csrf: user.csrf_token, body: { scope: "all" } },
      );
      setApplied(
        `已重新分類 ${r.processed} 筆同步交易，保留 ${r.protected} 筆你手動分過的。`,
      );
      setAdded(null);
      await client.invalidateQueries();
    } catch (e) {
      setApplied((e as Error).message);
    } finally {
      setApplying(false);
    }
  }

  const ready = pattern.trim().replace(/\s/g, "").length >= 2;
  const pickable = all.filter((c) => !c.hidden);

  return (
    <>
      <div className="rules-how">
        <Sparkles size={16} />
        <p>
          銀行同步進來的交易，會看<b>說明文字裡有沒有某個關鍵字</b>
          來自動分類：先比對「我的規則」，沒有才用內建關鍵字。你手動改過分類的交易不會被改掉。
        </p>
      </div>

      <section className="rules-add">
        <h3>新增規則</h3>
        <label className="search">
          <WandSparkles size={16} />
          <input
            value={pattern}
            maxLength={80}
            placeholder="說明裡含有…（例如：全聯、Netflix、房租）"
            onChange={(e) => {
              setPattern(e.target.value);
              setError("");
            }}
          />
          {pattern && (
            <button
              type="button"
              aria-label="清除"
              onClick={() => setPattern("")}
            >
              <X size={14} />
            </button>
          )}
        </label>
        <FormError>{error}</FormError>
        {ready && (
          <>
            <p className="dim small">
              點一個分類就完成：含「{pattern.trim()}」的交易都歸到…
            </p>
            <div className="rules-cats">
              {(["expense", "income", "flow"] as const).map((k) => (
                <div key={k}>
                  <span className="rules-cats-label">{kindLabels[k]}</span>
                  <div className="rules-cats-grid">
                    {pickable
                      .filter((c) => c.kind === k)
                      .map((c) => (
                        <button
                          type="button"
                          key={c.id}
                          disabled={busy}
                          onClick={() => add(c.name)}
                        >
                          <CategoryIcon category={c.name} size={22} />
                          {c.name}
                        </button>
                      ))}
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
        {added && (
          <div className="rules-apply">
            <span>
              新規則只會套用到之後同步的交易。要不要把過去的交易也重新分類？
            </span>
            <Button
              size="sm"
              variant="primary"
              busy={applying}
              onClick={applyAll}
            >
              套用到所有舊交易
            </Button>
          </div>
        )}
        {applied && <p className="small rules-applied">{applied}</p>}
      </section>

      <label className="search rules-search">
        <Search size={16} />
        <input
          value={q}
          placeholder="搜尋關鍵字或分類"
          onChange={(e) => setQ(e.target.value)}
        />
        {q && (
          <button type="button" aria-label="清除搜尋" onClick={() => setQ("")}>
            <X size={14} />
          </button>
        )}
      </label>

      <section>
        <h3 className="rules-title">
          我的規則{" "}
          <span className="dim small">
            {rules.data?.length ?? 0} 條 · 優先使用
          </span>
        </h3>
        {rules.isPending ? (
          <Skeleton rows={2} />
        ) : rules.isError ? (
          <ErrorLine retry={() => rules.refetch()}>規則讀取失敗</ErrorLine>
        ) : !mine.length ? (
          <Empty
            compact
            icon={WandSparkles}
            title={needle ? "沒有符合的規則" : "還沒有自己的規則"}
          >
            {!needle &&
              "在上面輸入關鍵字，或在收支頁改一筆交易的分類時勾選「以後自動分類」。"}
          </Empty>
        ) : (
          <div className="rules-groups">
            {mine.map(([cat, items]) => (
              <div key={cat} className="rules-group">
                <div className="rules-group-head">
                  <CategoryIcon category={cat} size={26} />
                  <strong>{cat}</strong>
                  <span className="dim small">{items.length}</span>
                </div>
                <div className="kw-list">
                  {items.map((r) => (
                    <span key={r.id} className="kw mine">
                      {r.pattern}
                      {r.matches > 0 && (
                        <small title={`目前有 ${r.matches} 筆交易符合`}>
                          {r.matches}
                        </small>
                      )}
                      <button
                        type="button"
                        aria-label={`刪除規則「${r.pattern}」`}
                        disabled={busy}
                        onClick={() =>
                          run(`/category-rules/${r.id}`, {
                            method: "DELETE",
                            success: `已刪除「${r.pattern}」`,
                            toastError: true,
                          })
                        }
                      >
                        <X size={12} />
                      </button>
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <details className="rules-builtin" open={needle ? true : undefined}>
        <summary>
          內建關鍵字{" "}
          <span className="dim small">
            {defaults.data?.length ?? "…"} 個 · 大家共用，無法修改
          </span>
        </summary>
        {defaults.isError ? (
          <ErrorLine retry={() => defaults.refetch()}>
            內建關鍵字讀取失敗
          </ErrorLine>
        ) : !builtin.length ? (
          <p className="dim small">沒有符合的內建關鍵字</p>
        ) : (
          <div className="rules-groups">
            {builtin.map(([cat, items]) => (
              <div key={cat} className="rules-group">
                <div className="rules-group-head">
                  <CategoryIcon category={cat} size={26} />
                  <strong>{cat}</strong>
                  <span className="dim small">{items.length}</span>
                </div>
                <div className="kw-list">
                  {items.map((r) => (
                    <span
                      key={r.pattern}
                      className="kw"
                      title={
                        r.kind === "income"
                          ? "只用在收入"
                          : r.kind === "expense"
                            ? "只用在支出"
                            : "收入與支出都適用"
                      }
                    >
                      {r.pattern}
                      {r.kind === "income" && <small>收入</small>}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
        <p className="dim small">
          想讓某個內建關鍵字歸到別的分類？新增一條同樣關鍵字的「我的規則」就會優先使用。
        </p>
      </details>

      <details className="rules-builtin">
        <summary>把規則套用到過去的交易</summary>
        <ReclassifyForm disabled={busy} />
      </details>
    </>
  );
}
