"use client";
import { createContext, useContext, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  api,
  type Account,
  type Budget,
  type Dashboard,
  type Entry,
  type FxQuotes,
  type Goal,
  type Job,
  type NetWorth,
  type SourceAsset,
  type SyncEvent,
} from "./api";
import { activeJob } from "./format";

export const useAccounts = () =>
  useQuery({
    queryKey: ["accounts"],
    queryFn: () => api<Account[]>("/accounts"),
  });
export const useDashboard = () =>
  useQuery({
    queryKey: ["dashboard"],
    queryFn: () => api<Dashboard>("/dashboard"),
  });
export const useRecent = () =>
  useQuery({
    queryKey: ["transactions", "recent"],
    queryFn: () => api<Entry[]>("/activity-feed"),
  });
export const useNetWorth = () =>
  useQuery({
    queryKey: ["net-worth"],
    queryFn: () => api<NetWorth>("/net-worth"),
  });
/** 外幣兌台幣匯率；後端 Redis 快取 10 分鐘 */
export const useFx = (enabled = true) =>
  useQuery({
    queryKey: ["fx"],
    queryFn: () => api<FxQuotes | null>("/fx"),
    staleTime: 5 * 60_000,
    enabled,
  });
export const useSourceAssets = () =>
  useQuery({
    queryKey: ["source-assets"],
    queryFn: () => api<SourceAsset[]>("/source-assets"),
  });
export const useBudgets = () =>
  useQuery({ queryKey: ["budgets"], queryFn: () => api<Budget[]>("/budgets") });
export const useGoals = () =>
  useQuery({ queryKey: ["goals"], queryFn: () => api<Goal[]>("/goals") });

/** 同步工作：SSE 即時推送，斷線時退回每 5 秒輪詢 */
export function useJobs(enabled: boolean) {
  const client = useQueryClient();
  const [live, setLive] = useState(false);
  const jobs = useQuery({
    queryKey: ["sync-jobs"],
    queryFn: () => api<Job[]>("/sync/jobs"),
    enabled,
    refetchInterval: live ? false : 5000,
  });
  useEffect(() => {
    if (!enabled) return;
    const source = new EventSource("/api/v1/sync/events");
    source.addEventListener("jobs", (e) => {
      try {
        client.setQueryData(
          ["sync-jobs"],
          JSON.parse((e as MessageEvent).data),
        );
        setLive(true);
      } catch {
        setLive(false);
      }
    });
    source.onerror = () => setLive(false);
    return () => source.close();
  }, [client, enabled]);
  return { ...jobs, live };
}

/**
 * 單一同步工作的 log：進行中以 SSE 即時接收（斷線由瀏覽器帶 Last-Event-ID 接續），
 * SSE 不可用時每 2 秒輪詢；已結束的工作一次讀回歷史。
 */
export function useJobLog(jobId: string | undefined, active: boolean) {
  const [events, setEvents] = useState<SyncEvent[]>([]);
  const [live, setLive] = useState(false);
  const [error, setError] = useState(false);
  // 工作從進行中轉為結束時沿用已收到的事件，只補讀後續
  const cursor = useRef<{ job?: string; last: number }>({ last: 0 });
  useEffect(() => {
    if (cursor.current.job !== jobId) {
      cursor.current = { job: jobId, last: 0 };
      setEvents([]);
    }
    setError(false);
    setLive(false);
    if (!jobId) return;
    const c = cursor.current;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const add = (items: SyncEvent[]) => {
      const fresh = items.filter((e) => e.id > c.last);
      if (!fresh.length) return;
      c.last = fresh[fresh.length - 1].id;
      setEvents((prev) => [...prev, ...fresh]);
    };
    const pull = async (): Promise<boolean> => {
      // 每次最多 200 筆；讀滿就繼續讀，直到追上最新
      for (;;) {
        const page = await api<{
          job: { status: string };
          events: SyncEvent[];
        }>(`/sync/jobs/${jobId}/log?after=${c.last}`);
        add(page.events);
        if (page.events.length < 200) return !activeJob(page.job.status);
      }
    };
    const poll = () => {
      if (stopped) return;
      pull()
        .then((finished) => {
          setError(false);
          if (!finished && !stopped) timer = setTimeout(poll, 2000);
        })
        .catch(() => {
          setError(true);
          if (!stopped) timer = setTimeout(poll, 5000);
        });
    };
    if (!active) {
      poll();
      return () => {
        stopped = true;
        clearTimeout(timer);
      };
    }
    const source = new EventSource(
      `/api/v1/sync/jobs/${jobId}/events?after=${c.last}`,
    );
    source.addEventListener("log", (e) => {
      try {
        add([JSON.parse((e as MessageEvent).data)]);
        setLive(true);
        setError(false);
      } catch {
        // 單筆格式錯誤不中斷串流
      }
    });
    source.addEventListener("job", () => setLive(true));
    source.addEventListener("done", () => source.close());
    source.onerror = () => {
      setLive(false);
      // 瀏覽器放棄重連時改為輪詢
      if (source.readyState === EventSource.CLOSED && !timer) poll();
    };
    return () => {
      stopped = true;
      source.close();
      clearTimeout(timer);
    };
  }, [jobId, active]);
  return { events, live, error };
}

export const JobsContext = createContext<{
  jobs: Job[];
  live: boolean;
  error: boolean;
}>({
  jobs: [],
  live: false,
  error: false,
});
export const useJobsContext = () => useContext(JobsContext);
