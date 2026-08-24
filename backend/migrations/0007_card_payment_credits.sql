-- C6 card fatura: "Pagamento Fatura" credits are card payments, not refunds.
UPDATE items SET kind = 'card_payment', amount_cents = -abs(amount_cents)
WHERE kind = 'refund' AND description ILIKE '%pagamento fatura%';
