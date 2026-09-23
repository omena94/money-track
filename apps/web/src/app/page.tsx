"use client";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError, type Health, type User } from "@/lib/api";
import { Login } from "@/features/login";
import { Logo, Workspace } from "@/features/shell";

export default function Home() {
  const user = useQuery({
    queryKey: ["me"],
    queryFn: () => api<User>("/auth/me"),
    // 可見頁面保持登入時持續記錄活動；登入失效後停止輪詢。
    refetchInterval: (query) =>
      query.state.data &&
      !(
        query.state.error instanceof ApiError &&
        query.state.error.status === 401
      )
        ? 30000
        : false,
    refetchOnWindowFocus: "always",
  });
  const health = useQuery({
    queryKey: ["health"],
    queryFn: () => api<Health>("/health"),
  });
  if (user.isPending)
    return (
      <main className="splash" aria-busy="true">
        <Logo size={44} />
        <p>正在開啟…</p>
      </main>
    );
  if (
    user.error &&
    (!user.data ||
      (user.error instanceof ApiError && user.error.status === 401))
  ) {
    if (user.error instanceof ApiError && user.error.status === 401)
      return (
        <Login
          configured={health.data?.google_configured ?? false}
          loginUrl={health.data?.login_url ?? "/api/v1/auth/google"}
          connected={health.data?.status === "ok"}
        />
      );
    return (
      <main className="splash">
        <Logo size={44} />
        <h1>暫時連不上服務</h1>
        <p>請確認 Money Track 已在主機上啟動，稍後再試一次。</p>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => user.refetch()}
        >
          重新連線
        </button>
      </main>
    );
  }
  return <Workspace user={user.data} health={health.data} />;
}
