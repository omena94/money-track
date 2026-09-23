import type { MetadataRoute } from "next";

export const dynamic = "force-static";

/** 加到手機主畫面時的名稱、圖示與顏色 */
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "Money Track",
    short_name: "Money",
    description: "個人與家庭的本地財務工作台",
    start_url: "/",
    display: "standalone",
    background_color: "#121314",
    theme_color: "#121314",
    icons: [
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml" },
      { src: "/apple-icon", sizes: "180x180", type: "image/png" },
    ],
  };
}
