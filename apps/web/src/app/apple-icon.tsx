import { ImageResponse } from "next/og";

// iOS 主畫面與 PWA 用的 PNG 圖示，與 icon.svg 同一設計（iOS 會自行裁圓角，這裡用滿版底色）
export const size = { width: 180, height: 180 };
export const contentType = "image/png";
export const dynamic = "force-static";

const mark = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3cc49c"/><stop offset="1" stop-color="#17684f"/></linearGradient></defs>
<rect width="64" height="64" fill="url(#bg)"/>
<path d="M15 46V22l11 12.5 6.5-7.5 7 8L50 22" fill="none" stroke="#fff" stroke-width="6.4" stroke-linecap="round" stroke-linejoin="round"/>
<path d="M40.5 21.5H50.5V31.5" fill="none" stroke="#fff" stroke-width="6.4" stroke-linecap="round" stroke-linejoin="round"/>
</svg>`;

export default function AppleIcon() {
  return new ImageResponse(
    (
      // eslint-disable-next-line @next/next/no-img-element
      <img
        width={180}
        height={180}
        alt=""
        src={`data:image/svg+xml;base64,${Buffer.from(mark).toString("base64")}`}
      />
    ),
    size,
  );
}
