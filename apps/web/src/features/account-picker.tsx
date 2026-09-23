"use client";
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import Decimal from "decimal.js";
import { Check, ChevronDown, Layers, Search, X } from "lucide-react";
import type { Account, SourceAsset } from "@/lib/api";
import { kinds, sourceNames } from "@/lib/format";
import { BankLogo } from "@/components/bank-logo";
import { Money } from "@/components/ui";
import { kindIcons, kindTones } from "./account-form";
import { accountTypes } from "./asset-sheet";

type Option = {
  id: string;
  name: string;
  meta: string;
  amount: string | null | undefined;
  currency: string;
  names: (string | null | undefined)[];
  kind?: string;
  search: string;
};
type Group = { title: string; options: Option[] };

/** 全形英數轉半形，「ｉＬＥＯ帳戶」顯示為「iLEO帳戶」 */
const tidy = (s: string) => s.normalize("NFKC").replace(/\s+/g, " ").trim();
const norm = (s: string) => tidy(s).toLowerCase();

function buildGroups(accounts: Account[], sources: SourceAsset[]): Group[] {
  const map = new Map<string, Group>();
  const add = (title: string, o: Option) => {
    const g = map.get(title) ?? { title, options: [] };
    g.options.push(o);
    map.set(title, g);
  };
  for (const a of accounts) {
    const title = a.institution?.trim() || "手動帳戶";
    const name = tidy(a.name);
    add(title, {
      id: a.id,
      name,
      meta: [kinds[a.kind], a.currency !== "TWD" && a.currency, "手動"]
        .filter(Boolean)
        .join(" · "),
      amount: a.balance,
      currency: a.currency,
      names: [a.institution],
      kind: a.kind,
      search: norm(`${title} ${name} ${kinds[a.kind] ?? ""}`),
    });
  }
  for (const s of sources) {
    if (s.kind !== "account") continue;
    const title = sourceNames[s.connector] ?? s.institution ?? s.connector;
    const name = tidy(s.name);
    const mask = s.mask && !name.includes(s.mask) ? s.mask : null;
    const type = accountTypes[s.account_type ?? ""];
    add(title, {
      id: s.id,
      name,
      meta: [mask, type, s.currency !== "TWD" && s.currency, "同步"]
        .filter(Boolean)
        .join(" · "),
      amount:
        s.amount != null && s.category === "debt"
          ? new Decimal(s.amount).abs().negated().toString()
          : s.amount,
      currency: s.currency,
      names: [s.institution, title, s.connector],
      search: norm(`${title} ${name} ${s.mask ?? ""} ${type ?? ""}`),
    });
  }
  return [...map.values()];
}

const Logo = ({ o, size }: { o: Option; size: number }) => (
  <BankLogo
    names={o.names}
    size={size}
    fallback={o.kind ? kindIcons[o.kind] : undefined}
    tone={o.kind ? kindTones[o.kind] : undefined}
  />
);

/** 收支頁帳戶篩選：依金融機構分組，可搜尋、鍵盤選取 */
export function AccountPicker({
  accounts,
  sources,
  value,
  onChange,
}: {
  accounts: Account[];
  sources: SourceAsset[];
  value: string;
  onChange: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const wrap = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listId = useId();

  const groups = useMemo(
    () => buildGroups(accounts, sources),
    [accounts, sources],
  );
  const selected = useMemo(
    () => groups.flatMap((g) => g.options).find((o) => o.id === value),
    [groups, value],
  );
  const shown = useMemo(() => {
    const words = norm(q).split(" ").filter(Boolean);
    if (!words.length) return groups;
    return groups
      .map((g) => ({
        ...g,
        options: g.options.filter((o) =>
          words.every((w) => o.search.includes(w)),
        ),
      }))
      .filter((g) => g.options.length);
  }, [groups, q]);
  // 鍵盤游標走訪的順序：「所有帳戶」在前（搜尋時隱藏），再依分組
  const flat = useMemo(
    () => [
      ...(q ? [] : [""]),
      ...shown.flatMap((g) => g.options.map((o) => o.id)),
    ],
    [shown, q],
  );

  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  const toggle = () => {
    if (!open) {
      // 開啟時清空搜尋並把游標放在目前選取的帳戶
      setQ("");
      setCursor(
        Math.max(
          0,
          ["", ...groups.flatMap((g) => g.options.map((o) => o.id))].indexOf(
            value,
          ),
        ),
      );
    }
    setOpen(!open);
  };
  useEffect(() => {
    list.current
      ?.querySelector<HTMLElement>(`[data-index="${cursor}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor, open]);

  const pick = (id: string) => {
    onChange(id);
    setOpen(false);
    trigger.current?.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const d = e.key === "ArrowDown" ? 1 : -1;
      setCursor((c) => (c + d + flat.length) % Math.max(flat.length, 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (flat[cursor] !== undefined) pick(flat[cursor]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      trigger.current?.focus();
    }
  };

  let index = q ? 0 : 1;
  return (
    <div className="acct-picker" ref={wrap}>
      <button
        ref={trigger}
        type="button"
        className={`acct-picker-trigger ${open ? "open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="篩選帳戶"
        onClick={toggle}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "Enter")) {
            e.preventDefault();
            toggle();
          }
        }}
      >
        {selected ? (
          <Logo o={selected} size={24} />
        ) : (
          <span className="acct-picker-all">
            <Layers size={14} />
          </span>
        )}
        <span className="acct-picker-label">
          {selected?.name ?? "所有帳戶"}
        </span>
        <ChevronDown size={16} className="acct-picker-chevron" />
      </button>
      {selected && (
        <button
          type="button"
          className="acct-picker-clear"
          aria-label="清除帳戶篩選"
          onClick={() => onChange("")}
        >
          <X size={14} />
        </button>
      )}
      {open && (
        <div className="acct-picker-panel" onKeyDown={onKey}>
          <label className="acct-picker-search">
            <Search size={16} />
            <input
              autoFocus
              type="search"
              placeholder="搜尋帳戶、銀行或末碼"
              aria-label="搜尋帳戶"
              aria-controls={listId}
              aria-activedescendant={
                flat[cursor] !== undefined ? `${listId}-${cursor}` : undefined
              }
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                setCursor(0);
              }}
            />
          </label>
          <div
            className="acct-picker-list"
            role="listbox"
            id={listId}
            ref={list}
            aria-label="帳戶"
          >
            {!q && (
              <Row
                id={`${listId}-0`}
                index={0}
                active={cursor === 0}
                selected={!value}
                onPick={() => pick("")}
                onHover={() => setCursor(0)}
              >
                <span className="acct-picker-all lg">
                  <Layers size={16} />
                </span>
                <span className="acct-picker-main">
                  <strong>所有帳戶</strong>
                  <small>
                    {groups.reduce((n, g) => n + g.options.length, 0)} 個帳戶
                  </small>
                </span>
              </Row>
            )}
            {shown.map((g) => (
              <div key={g.title} role="group" aria-label={g.title}>
                <div className="acct-picker-group">
                  <span>{g.title}</span>
                  <span>{g.options.length}</span>
                </div>
                {g.options.map((o) => {
                  const i = index++;
                  return (
                    <Row
                      key={o.id}
                      id={`${listId}-${i}`}
                      index={i}
                      active={cursor === i}
                      selected={o.id === value}
                      onPick={() => pick(o.id)}
                      onHover={() => setCursor(i)}
                    >
                      <Logo o={o} size={32} />
                      <span className="acct-picker-main">
                        <strong>{o.name}</strong>
                        <small>{o.meta}</small>
                      </span>
                      <Money
                        value={o.amount}
                        currency={o.currency}
                        className="acct-picker-amount"
                      />
                    </Row>
                  );
                })}
              </div>
            ))}
            {!shown.length && (
              <p className="acct-picker-empty">找不到符合「{q}」的帳戶</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function Row({
  id,
  index,
  active,
  selected,
  onPick,
  onHover,
  children,
}: {
  id: string;
  index: number;
  active: boolean;
  selected: boolean;
  onPick: () => void;
  onHover: () => void;
  children: ReactNode;
}) {
  return (
    <div
      id={id}
      role="option"
      aria-selected={selected}
      data-index={index}
      className={`acct-picker-option ${active ? "active" : ""} ${selected ? "selected" : ""}`}
      onPointerMove={onHover}
      onClick={onPick}
    >
      {children}
      <Check size={16} className="acct-picker-check" />
    </div>
  );
}
