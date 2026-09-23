import { describe, expect, it } from "vitest";
import { scheduledPaymentAmount } from "../src/lib/payment-reminders";

describe("scheduled payment reminders", () => {
  it("uses the next loan installment, never the outstanding principal", () => {
    expect(scheduledPaymentAmount({ account_type: "loan", amount: "-14000000", next_payment_amount: "38500" })).toBe("38500");
    expect(scheduledPaymentAmount({ account_type: "loan", amount: "-14000000", next_payment_amount: null })).toBeNull();
    expect(scheduledPaymentAmount({ account_type: "mortgage", amount: "-14000000", balance_from: "bill" })).toBeNull();
  });
  it("accepts a card balance only when explicitly derived from its bill", () => {
    expect(scheduledPaymentAmount({ account_type: "credit", amount: "-16000", balance_from: "bill" })).toBe("16000");
    expect(scheduledPaymentAmount({ account_type: "credit", amount: "-16000" })).toBeNull();
  });
  it("preserves known zero and leaves malformed or negative installments unknown", () => {
    expect(scheduledPaymentAmount({ account_type: "loan", amount: "-1000", next_payment_amount: "0.00" })).toBe("0");
    for (const value of ["", "NaN", "Infinity", "-25", "1e10"]) {
      expect(scheduledPaymentAmount({ account_type: "loan", amount: "-1000", next_payment_amount: value })).toBeNull();
    }
  });
});
