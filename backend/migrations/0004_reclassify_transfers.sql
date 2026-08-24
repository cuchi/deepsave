-- Transfers are no longer a distinct kind: Pix/TED sent = expense, received = income.
UPDATE items SET kind = 'expense' WHERE kind = 'transfer_out';
UPDATE items SET kind = 'income' WHERE kind = 'transfer_in';
