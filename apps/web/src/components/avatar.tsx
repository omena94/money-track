"use client";
import { useState } from "react";
import type { AvatarValue } from "@/lib/api";
import { colorVar } from "@/lib/categories";

export type { AvatarValue };

export type AvatarPerson = {
  id: string;
  name: string;
  avatar?: AvatarValue | null;
};

/** 使用者頭像：照片、表情符號或名稱首字；沒有設定時用首字＋品牌色 */
export function Avatar({
  person,
  size = 34,
  className = "",
}: {
  person: AvatarPerson;
  size?: number;
  className?: string;
}) {
  const a = person.avatar;
  const [broken, setBroken] = useState(false);
  const style = {
    width: size,
    height: size,
    fontSize: Math.round(size * (a?.type === "emoji" ? 0.55 : 0.42)),
    ...(a && a.type !== "photo" && a.color
      ? {
          background: `color-mix(in srgb, ${colorVar(a.color)} 20%, transparent)`,
          color: colorVar(a.color),
        }
      : {}),
  };
  if (a?.type === "photo" && !broken)
    return (
      // eslint-disable-next-line @next/next/no-img-element
      <img
        className={`avatar photo ${className}`}
        src={`/api/v1/users/${person.id}/avatar?v=${encodeURIComponent(a.v)}`}
        alt=""
        aria-hidden="true"
        width={size}
        height={size}
        style={{ width: size, height: size }}
        onError={() => setBroken(true)}
      />
    );
  return (
    <span className={`avatar ${className}`} style={style} aria-hidden="true">
      {a?.type === "emoji" ? a.emoji : ([...person.name.trim()][0] ?? "?")}
    </span>
  );
}
