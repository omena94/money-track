"use client";
import { useState } from "react";
import { Landmark, type LucideIcon } from "lucide-react";

/** 機構名稱關鍵字 → public/banks/{key}.png（各銀行官方 App 圖示） */
const rules: [RegExp, string][] = [
  [/國泰|cathay/i, "cathaybk"],
  [/富邦|fubon/i, "fubon"],
  [/永豐|sinopac/i, "sinopac"],
  [/第一銀|第一商|first ?bank/i, "firstbank"],
  [/玉山|e\.?sun/i, "esun"],
  [/中國信託|中信銀|ctbc/i, "ctbc"],
  [/richart/i, "richart"],
  [/台新|taishin/i, "taishin"],
  [/兆豐|mega/i, "mega"],
  [/合作金庫|合庫|cooperative/i, "tcb"],
  [/華南|hua ?nan|hncb/i, "hncb"],
  [/彰化|彰銀|chang ?hwa/i, "chb"],
  [/土地銀|土銀|land ?bank/i, "landbank"],
  [/[臺台]灣企銀|[臺台]企銀|business bank/i, "tbb"],
  [/[臺台]灣銀行|[臺台]銀|bank of taiwan/i, "bot"],
  [/聯邦|union bank/i, "ubot"],
  [/元大|yuanta/i, "yuanta"],
  [/凱基|kgi/i, "kgi"],
  [/新光|shin ?kong|skbank/i, "skbank"],
  [/上海商|scsb/i, "scsb"],
  [/遠東|far eastern|feib|bankee/i, "feib"],
  [/王道|o-?bank/i, "obank"],
  [/line ?bank|連線銀行/i, "linebank"],
  [/將來|next ?bank/i, "nextbank"],
  [/樂天|rakuten/i, "rakuten"],
  [/郵局|郵政|chunghwa post/i, "post"],
  [/集保|tdcc/i, "tdcc"],
  [/星展|dbs/i, "dbs"],
  [/[滙匯]豐|hsbc/i, "hsbc"],
  [/渣打|standard chartered/i, "sc"],
  [/陽信|sunny/i, "sunny"],
  [/高雄銀|高銀|kaohsiung/i, "bok"],
];

export function bankKey(...names: (string | null | undefined)[]) {
  for (const n of names) {
    if (!n) continue;
    for (const [re, key] of rules) if (re.test(n)) return key;
  }
  return null;
}

/**
 * 銀行圖示：認得的機構顯示官方圖示，不認得就退回一般圖示。
 * `names` 依序比對，例如 [機構名稱, 來源 id]。
 */
export function BankLogo({
  names,
  size = 36,
  fallback: Fallback = Landmark,
  tone,
  className = "",
}: {
  names: (string | null | undefined)[];
  size?: number;
  fallback?: LucideIcon;
  /** 退回圖示的主色；未給時為中性灰 */
  tone?: string;
  className?: string;
}) {
  const key = bankKey(...names);
  const [broken, setBroken] = useState(false);
  if (!key || broken)
    return (
      <span
        className={`bank-logo fallback ${tone ? "tinted" : ""} ${className}`}
        style={{
          width: size,
          height: size,
          ...(tone && { ["--tone" as string]: tone }),
        }}
        aria-hidden="true"
      >
        <Fallback size={Math.round(size * 0.5)} />
      </span>
    );
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      className={`bank-logo ${className}`}
      src={`/banks/${key}.png`}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      loading="lazy"
      onError={() => setBroken(true)}
    />
  );
}

const tones = [
  "var(--c1)",
  "var(--c2)",
  "var(--c3)",
  "var(--c4)",
  "var(--c5)",
  "var(--c6)",
  "var(--c7)",
  "var(--c8)",
];

/**
 * 持股圖示：名稱含認得的發行商（元大、富邦…）時用其圖示，
 * 否則以名稱首字配上依名稱固定的顏色，讓每檔一眼可分。
 */
export function HoldingLogo({
  name,
  size = 40,
}: {
  name: string;
  size?: number;
}) {
  if (bankKey(name)) return <BankLogo names={[name]} size={size} />;
  const label = [...name.trim()][0] ?? "?";
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return (
    <span
      className="bank-logo fallback tinted initial"
      style={{
        width: size,
        height: size,
        fontSize: Math.round(size * 0.42),
        ["--tone" as string]: tones[h % tones.length],
      }}
      aria-hidden="true"
    >
      {label}
    </span>
  );
}
