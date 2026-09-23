import Decimal from "decimal.js";
import type { SourceAsset } from "./api";

type PaymentAsset = Pick<SourceAsset, "account_type" | "next_payment_amount" | "balance_from" | "amount">;

/** Loan principal and current card balances are not scheduled payment amounts. */
export function scheduledPaymentAmount(asset: PaymentAsset): string | null {
  const loan = asset.account_type === "loan" || asset.account_type === "mortgage";
  const fromBill = !loan && asset.balance_from === "bill";
  const value = fromBill ? asset.amount : asset.next_payment_amount;
  if (value == null || !/^-?\d+(?:\.\d+)?$/.test(value)) return null;
  const amount = new Decimal(value);
  if (!amount.isFinite() || (!fromBill && amount.isNegative())) return null;
  return (fromBill ? amount.abs() : amount).toString();
}
