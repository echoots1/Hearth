/** Typed hooks over the registry, one per screen need. */
import { tool } from "./api";
import { useQuery } from "./query";
import type {
  Account, Alert, AssistantAction, BalanceOverview, Bill, Category, CategoryDetail, DebtComparison, DebtProjection,
  Debt, Goal, Group, MonthView, PlannedItem, Question, ReviewItem, Rule, Settings, SinkingOverview, Today, Transaction, Page,
} from "./types";

export const useToday = () => useQuery<Today>("household.today", () => tool("household.today"));
export const useSettings = () => useQuery<Settings>("household.settings", () => tool("household.settings"));
export const useMonth = (month?: string) =>
  useQuery<MonthView>(`budget.month:${month ?? ""}`, () => tool("budget.month", month ? { month } : {}));
export const useCategoryDetail = (id: string | undefined, month?: string) =>
  useQuery<CategoryDetail>(id ? `budget.category:${id}:${month ?? ""}` : null, () => tool("budget.category", { id, month }));
export const useGroups = () =>
  useQuery<{ groups: Group[]; categories: Category[] }>("budget.groups", () => tool("budget.groups"));
export const useBalance = () => useQuery<BalanceOverview>("balance.overview", () => tool("balance.overview"));
export const useAccounts = (inactive = false) =>
  useQuery<Account[]>(`accounts.list:${inactive}`, () => tool("accounts.list", { include_inactive: inactive }));
export const useReviewQueue = () => useQuery<ReviewItem[]>("transactions.review_queue", () => tool("transactions.review_queue"));
export const useTransactions = (args: Record<string, unknown>) =>
  useQuery<Page<Transaction>>(`transactions.list:${JSON.stringify(args)}`, () => tool("transactions.list", args));
export const useBills = (args: Record<string, unknown> = {}) =>
  useQuery<Bill[]>(`bills.list:${JSON.stringify(args)}`, () => tool("bills.list", args));
export const usePlanned = (args: Record<string, unknown> = {}) =>
  useQuery<PlannedItem[]>(`planned.list:${JSON.stringify(args)}`, () => tool("planned.list", args));
export const useSinking = () => useQuery<SinkingOverview>("sinking.overview", () => tool("sinking.overview"));
export const useDebts = () => useQuery<Debt[]>("debts.list", () => tool("debts.list"));
export const useDebtProjection = () => useQuery<DebtProjection>("debts.projection", () => tool("debts.projection"));
export const useDebtComparison = () => useQuery<DebtComparison>("debts.compare", () => tool("debts.compare"));
export const useGoals = () => useQuery<Goal[]>("goals.list", () => tool("goals.list"));
export const useAlerts = (args: Record<string, unknown> = {}) =>
  useQuery<Alert[]>(`alerts.list:${JSON.stringify(args)}`, () => tool("alerts.list", args));
export const useQuestions = (args: Record<string, unknown> = {}) =>
  useQuery<Question[]>(`questions.list:${JSON.stringify(args)}`, () => tool("questions.list", args));
export const useActions = (args: Record<string, unknown> = {}) =>
  useQuery<AssistantAction[]>(`assistant.actions:${JSON.stringify(args)}`, () => tool("assistant.actions", args));
export const useRules = (inactive = false) =>
  useQuery<Rule[]>(`rules.list:${inactive}`, () => tool("rules.list", { include_inactive: inactive }));

/** Open-item counts for the attention row and nav badge. */
export function useInboxCounts() {
  const alerts = useAlerts({ live: true });
  const questions = useQuestions({ status: "open" });
  const review = useReviewQueue();
  const a = alerts.data ?? [];
  return {
    alerts: a.length,
    critical: a.some((x) => x.severity === "critical"),
    questions: (questions.data ?? []).length,
    review: (review.data ?? []).length,
    total: a.length + (questions.data ?? []).length + (review.data ?? []).length,
  };
}
