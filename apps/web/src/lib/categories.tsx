"use client";
import { createContext, useContext, useMemo, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Apple,
  ArrowLeftRight,
  Baby,
  Banknote,
  Beer,
  Bike,
  BookOpen,
  Bot,
  Briefcase,
  Building2,
  Bus,
  CakeSlice,
  Camera,
  Car,
  Cat,
  Cigarette,
  CircleDashed,
  Clapperboard,
  Cloud,
  Coffee,
  Coins,
  CreditCard,
  Dog,
  Droplet,
  Dumbbell,
  Film,
  Flower2,
  Fuel,
  Gamepad2,
  Gem,
  Gift,
  Globe,
  GraduationCap,
  Hammer,
  HandCoins,
  Headphones,
  Heart,
  HeartPulse,
  House,
  Key,
  Lamp,
  Landmark,
  Laptop,
  Music,
  ParkingMeter,
  PartyPopper,
  PiggyBank,
  Pill,
  Pizza,
  Plane,
  Receipt,
  Repeat,
  Scissors,
  Shield,
  Shirt,
  ShoppingBag,
  ShoppingCart,
  Smartphone,
  Sofa,
  Sparkles,
  Sprout,
  Stethoscope,
  Tag,
  Ticket,
  TrainFront,
  TrendingUp,
  Tv,
  Umbrella,
  Users,
  Utensils,
  Wallet,
  Wifi,
  Wrench,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { api } from "./api";

export type CategoryKind = "expense" | "income" | "flow";
export type UserCategory = {
  id: string;
  name: string;
  kind: CategoryKind;
  icon: string;
  color: string;
  position: number;
  hidden: boolean;
  builtin: boolean;
  /** 我的規則數 */
  rules: number;
  /** 內建關鍵字數 */
  keywords: number;
};

/** 可選的分類圖示：鍵值存進資料庫，順序即選擇器上的排列 */
export const categoryIconSet: [string, LucideIcon][] = [
  ["utensils", Utensils],
  ["coffee", Coffee],
  ["pizza", Pizza],
  ["cake-slice", CakeSlice],
  ["beer", Beer],
  ["apple", Apple],
  ["shopping-bag", ShoppingBag],
  ["shopping-cart", ShoppingCart],
  ["shirt", Shirt],
  ["gem", Gem],
  ["gift", Gift],
  ["sparkles", Sparkles],
  ["bus", Bus],
  ["train-front", TrainFront],
  ["car", Car],
  ["fuel", Fuel],
  ["parking-meter", ParkingMeter],
  ["bike", Bike],
  ["plane", Plane],
  ["globe", Globe],
  ["house", House],
  ["building-2", Building2],
  ["sofa", Sofa],
  ["lamp", Lamp],
  ["zap", Zap],
  ["droplet", Droplet],
  ["wifi", Wifi],
  ["smartphone", Smartphone],
  ["laptop", Laptop],
  ["tv", Tv],
  ["cloud", Cloud],
  ["bot", Bot],
  ["clapperboard", Clapperboard],
  ["film", Film],
  ["music", Music],
  ["headphones", Headphones],
  ["gamepad-2", Gamepad2],
  ["ticket", Ticket],
  ["camera", Camera],
  ["dumbbell", Dumbbell],
  ["heart-pulse", HeartPulse],
  ["pill", Pill],
  ["stethoscope", Stethoscope],
  ["scissors", Scissors],
  ["baby", Baby],
  ["dog", Dog],
  ["cat", Cat],
  ["users", Users],
  ["heart", Heart],
  ["party-popper", PartyPopper],
  ["flower-2", Flower2],
  ["sprout", Sprout],
  ["cigarette", Cigarette],
  ["graduation-cap", GraduationCap],
  ["book-open", BookOpen],
  ["briefcase", Briefcase],
  ["shield", Shield],
  ["umbrella", Umbrella],
  ["landmark", Landmark],
  ["receipt", Receipt],
  ["banknote", Banknote],
  ["coins", Coins],
  ["wallet", Wallet],
  ["piggy-bank", PiggyBank],
  ["credit-card", CreditCard],
  ["hand-coins", HandCoins],
  ["trending-up", TrendingUp],
  ["arrow-left-right", ArrowLeftRight],
  ["repeat", Repeat],
  ["key", Key],
  ["wrench", Wrench],
  ["hammer", Hammer],
  ["tag", Tag],
];
const iconMap = new Map(categoryIconSet);
export const categoryColors = Array.from({ length: 12 }, (_, i) => `c${i + 1}`);
export const colorVar = (c: string) =>
  /^c([1-9]|1[0-2])$/.test(c) ? `var(--${c})` : "var(--ink-3)";

export const UNCATEGORIZED = "未分類";
/** 沒登入或尚未載入時使用的預設清單（與後端內建分類一致） */
const FALLBACK: Omit<
  UserCategory,
  "id" | "rules" | "keywords" | "hidden" | "builtin" | "position"
>[] = [
  { name: "餐飲", kind: "expense", icon: "utensils", color: "c1" },
  { name: "交通", kind: "expense", icon: "bus", color: "c2" },
  { name: "購物", kind: "expense", icon: "shopping-bag", color: "c3" },
  { name: "居家", kind: "expense", icon: "house", color: "c4" },
  { name: "娛樂", kind: "expense", icon: "clapperboard", color: "c5" },
  { name: "醫療", kind: "expense", icon: "heart-pulse", color: "c6" },
  { name: "教育", kind: "expense", icon: "graduation-cap", color: "c7" },
  { name: "保險", kind: "expense", icon: "shield", color: "c2" },
  { name: "稅費", kind: "expense", icon: "landmark", color: "c3" },
  { name: "手續費", kind: "expense", icon: "receipt", color: "c1" },
  { name: "電子支付", kind: "expense", icon: "smartphone", color: "c5" },
  { name: "提款", kind: "expense", icon: "banknote", color: "c6" },
  { name: "貸款", kind: "expense", icon: "hand-coins", color: "c4" },
  { name: "薪資", kind: "income", icon: "briefcase", color: "c8" },
  { name: "獎金", kind: "income", icon: "gift", color: "c1" },
  { name: "投資收益", kind: "income", icon: "trending-up", color: "c2" },
  { name: "其他收入", kind: "income", icon: "coins", color: "c3" },
  { name: "轉帳", kind: "flow", icon: "arrow-left-right", color: "c4" },
  { name: "信用卡繳款", kind: "flow", icon: "credit-card", color: "c7" },
  { name: "投資", kind: "flow", icon: "trending-up", color: "c8" },
];
const fallback: UserCategory[] = FALLBACK.map((c, i) => ({
  ...c,
  id: c.name,
  position: i,
  hidden: false,
  builtin: true,
  rules: 0,
  keywords: 0,
}));

// 目前使用者的分類，給 CategoryIcon / categoryColor 這類純函式查詢
let registry = new Map(fallback.map((c) => [c.name, c]));
export const findCategory = (name: string) => registry.get(name);
export const iconOf = (key: string) => iconMap.get(key);

export const useCategoryList = () =>
  useQuery({
    queryKey: ["categories"],
    queryFn: () => api<UserCategory[]>("/categories"),
    staleTime: 5 * 60_000,
  });

const Ctx = createContext<UserCategory[]>(fallback);

/** 放在應用程式最外層：載入分類並更新查詢表，分類變更時整個畫面跟著換圖示與顏色 */
export function CategoriesProvider({ children }: { children: ReactNode }) {
  const q = useCategoryList();
  const list = useMemo(() => {
    const l = q.data?.length ? q.data : fallback;
    registry = new Map(l.map((c) => [c.name, c]));
    return l;
  }, [q.data]);
  return <Ctx.Provider value={list}>{children}</Ctx.Provider>;
}

/** 全部分類（含隱藏） */
export const useCategories = () => useContext(Ctx);

/**
 * 選擇器用的分類名稱：指定類型中未隱藏者。
 * 收入與支出另外附上「不算收支」的資金移動分類。
 */
export function useCategoryNames(kind: "expense" | "income" | string) {
  const list = useCategories();
  return useMemo(() => {
    const main = list
      .filter(
        (c) =>
          !c.hidden && c.kind === (kind === "income" ? "income" : "expense"),
      )
      .map((c) => c.name);
    const flow = list
      .filter((c) => !c.hidden && c.kind === "flow")
      .map((c) => c.name);
    const hidden = list.filter((c) => c.hidden).map((c) => c.name);
    return { main, flow, hidden };
  }, [list, kind]);
}
