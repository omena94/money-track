"use client";
import { useRef, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { Camera, ImageUp, RotateCcw, Smile, Type } from "lucide-react";
import { api } from "@/lib/api";
import { categoryColors, colorVar } from "@/lib/categories";
import { useAction, useApp } from "@/components/app-context";
import { Avatar, type AvatarValue } from "@/components/avatar";
import {
  Button,
  Field,
  FormError,
  Segmented,
  Sheet,
  Skeleton,
} from "@/components/ui";

type Profile = {
  id: string;
  email: string;
  google_name: string;
  display_name: string | null;
  name: string;
  avatar: AvatarValue | null;
};
type Mode = "initial" | "emoji" | "photo";

const emojis = [
  "😀",
  "😎",
  "🥳",
  "🤓",
  "😺",
  "🐶",
  "🐱",
  "🐻",
  "🐼",
  "🦊",
  "🐯",
  "🐨",
  "🐰",
  "🐸",
  "🐧",
  "🦉",
  "🐳",
  "🦄",
  "🌸",
  "🌻",
  "🍀",
  "🌈",
  "⭐",
  "🔥",
  "⚡",
  "🍎",
  "🍜",
  "☕",
  "🎮",
  "🎸",
  "⚽",
  "🏀",
  "🚀",
  "✈️",
  "🏠",
  "💼",
  "💰",
  "📈",
  "🎯",
  "💎",
];
const PHOTO_SIZE = 256;

/** 把使用者選的照片裁成正方形並縮成 256px，轉成 WebP（不支援時用 JPEG） */
async function toSquare(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = PHOTO_SIZE;
  const ctx = canvas.getContext("2d")!;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(
    bitmap,
    (bitmap.width - side) / 2,
    (bitmap.height - side) / 2,
    side,
    side,
    0,
    0,
    PHOTO_SIZE,
    PHOTO_SIZE,
  );
  bitmap.close();
  const webp = canvas.toDataURL("image/webp", 0.86);
  return webp.startsWith("data:image/webp")
    ? webp
    : canvas.toDataURL("image/jpeg", 0.86);
}

/** 個人資料：顯示名稱與頭像 */
export function ProfileSheet({ onClose }: { onClose: () => void }) {
  const profile = useQuery({
    queryKey: ["profile"],
    queryFn: () => api<Profile>("/profile"),
  });
  return (
    <Sheet
      title="個人資料"
      description="名稱與頭像會顯示在側欄、家庭與成員頁"
      onClose={onClose}
    >
      {profile.data ? (
        <ProfileForm profile={profile.data} onClose={onClose} />
      ) : (
        <Skeleton rows={3} />
      )}
    </Sheet>
  );
}

function ProfileForm({
  profile,
  onClose,
}: {
  profile: Profile;
  onClose: () => void;
}) {
  const { toast, user } = useApp();
  const { run, busy, error, setError } = useAction();
  const current = profile.avatar;
  const [name, setName] = useState(profile.display_name ?? "");
  const [mode, setMode] = useState<Mode>(current?.type ?? "initial");
  const [emoji, setEmoji] = useState(
    current?.type === "emoji" ? current.emoji : "😺",
  );
  const [color, setColor] = useState(
    current && current.type !== "photo" && current.color ? current.color : "c7",
  );
  const [photo, setPhoto] = useState<string>();
  const file = useRef<HTMLInputElement>(null);

  const shownName = name.trim() || profile.google_name;
  const preview: AvatarValue | null =
    mode === "photo"
      ? current?.type === "photo" && !photo
        ? current
        : null
      : mode === "emoji"
        ? { type: "emoji", emoji, color }
        : { type: "initial", color };

  async function pick(f: File | undefined) {
    if (!f) return;
    if (!/^image\/(png|jpe?g|webp|gif|heic|heif|avif)$/.test(f.type))
      return setError("請選擇圖片檔（JPG、PNG、WebP）");
    try {
      setPhoto(await toSquare(f));
      setMode("photo");
      setError("");
    } catch {
      setError("這張圖片無法讀取，請換一張");
    }
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    if (mode === "photo" && !photo && current?.type !== "photo")
      return setError("請先選擇一張照片");
    const ok =
      (await run("/profile", {
        method: "PATCH",
        body: {
          display_name: name.trim(),
          ...(mode === "photo"
            ? {}
            : {
                avatar:
                  mode === "emoji"
                    ? { type: "emoji", emoji, color }
                    : { type: "initial", color },
              }),
        },
      })) &&
      (mode !== "photo" ||
        !photo ||
        (await run("/profile/photo", {
          method: "PUT",
          body: { data: photo },
        })));
    if (ok) {
      toast("個人資料已更新");
      onClose();
    }
  }

  return (
    <form className="form profile-form" onSubmit={save}>
      <div className="profile-preview">
        {mode === "photo" && photo ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            className="avatar photo"
            src={photo}
            alt=""
            width={88}
            height={88}
          />
        ) : (
          <Avatar
            person={{ id: user.id, name: shownName, avatar: preview }}
            size={88}
          />
        )}
        <strong>{shownName}</strong>
        <small className="dim">{profile.email}</small>
      </div>
      <FormError>{error}</FormError>

      <Field
        label="顯示名稱"
        hint={`留空就使用 Google 名稱「${profile.google_name}」；之後用 Google 登入也不會被覆蓋`}
      >
        <div className="profile-name">
          <input
            value={name}
            maxLength={40}
            placeholder={profile.google_name}
            onChange={(e) => setName(e.target.value)}
          />
          {name && (
            <button
              type="button"
              className="icon-btn sm"
              title="改回 Google 名稱"
              aria-label="改回 Google 名稱"
              onClick={() => setName("")}
            >
              <RotateCcw size={15} />
            </button>
          )}
        </div>
      </Field>

      <Field label="頭像">
        <Segmented
          label="頭像類型"
          value={mode}
          onChange={setMode}
          options={[
            {
              value: "initial",
              label: (
                <>
                  <Type size={14} /> 名字
                </>
              ),
            },
            {
              value: "emoji",
              label: (
                <>
                  <Smile size={14} /> 表情
                </>
              ),
            },
            {
              value: "photo",
              label: (
                <>
                  <Camera size={14} /> 照片
                </>
              ),
            },
          ]}
        />
      </Field>

      {mode === "emoji" && (
        <div className="emoji-grid" role="radiogroup" aria-label="表情符號">
          {emojis.map((e) => (
            <button
              type="button"
              role="radio"
              key={e}
              aria-checked={emoji === e}
              className={emoji === e ? "on" : ""}
              onClick={() => setEmoji(e)}
            >
              {e}
            </button>
          ))}
        </div>
      )}
      {mode !== "photo" && (
        <div className="color-row" role="radiogroup" aria-label="頭像底色">
          {categoryColors.map((c) => (
            <button
              type="button"
              role="radio"
              key={c}
              aria-checked={color === c}
              aria-label={`底色 ${c}`}
              className={color === c ? "on" : ""}
              style={{ background: colorVar(c) }}
              onClick={() => setColor(c)}
            />
          ))}
        </div>
      )}
      {mode === "photo" && (
        <div className="photo-pick">
          <input
            ref={file}
            type="file"
            accept="image/*"
            hidden
            onChange={(e) => pick(e.target.files?.[0])}
          />
          <Button icon={ImageUp} onClick={() => file.current?.click()}>
            {photo || current?.type === "photo" ? "換一張照片" : "選擇照片"}
          </Button>
          <small className="dim">
            會自動裁成正方形並縮小，只存在這台主機。
          </small>
        </div>
      )}

      <div className="form-actions">
        <Button onClick={onClose} disabled={busy}>
          取消
        </Button>
        <Button type="submit" variant="primary" busy={busy}>
          儲存
        </Button>
      </div>
    </form>
  );
}
