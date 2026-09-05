import copy
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import portfolio_ops as ops


def fixture():
    return {'accounts': [{'id': 'a', 'name': 'Fixture', 'account_suffix': '1234', 'cash': 10000, 'holdings': []}], 'transactions': []}


def holding(q=100):
    return {'code': '600000', 'name': 'Fixture', 'market': 'Shanghai', 'quantity': q, 'cost_price': 10,
            'available_quantity': 0, 'available_date': '2026-09-01'}


class PortfolioTests(unittest.TestCase):
    def test_update_idempotence_unknown_evidence_and_cash_account(self):
        p = fixture()
        batch = {'expected_hash': ops.state_hash(p), 'observed_at': '2026-09-01T10:00:00+08:00',
                 'accounts': [{'suffix': '1234', 'cash': 9000, 'holdings': [holding()]}]}
        updated = ops.apply_batch(p, batch)
        ledger = ops.build_ledger(updated, [])
        ops.validate_state(updated, ledger, [])
        self.assertEqual(ledger['rows'][0]['confidence'], '截图推断/待核')
        self.assertIsNone(ledger['rows'][0]['fees'])
        self.assertIsNone(ledger['rows'][0]['realized_pnl'])
        with self.assertRaises(ValueError): ops.apply_batch(updated, batch)
        self.assertEqual(ops.build_ledger(p, [])['summaries'][0]['cash'], 10000)
        self.assertEqual(ops.digest({'quantity': 100}), ops.digest({'quantity': 100.0}))
        extra = copy.deepcopy(updated['transactions'][0])
        extra.update(code='600001', id='extra')
        batch['transactions'] = [extra]
        with self.assertRaises(ValueError): ops.apply_batch(p, batch)

    def test_validator_rejects_missing_duplicate_and_invalid_fields(self):
        p = ops.apply_batch(fixture(), {'expected_hash': ops.state_hash(fixture()), 'observed_at': '2026-09-01T10:00:00+08:00',
            'accounts': [{'suffix': '1234', 'cash': 9000, 'holdings': [holding()]}]})
        for mutate in [lambda x: x['transactions'][0].update(kind='Invalid'),
                       lambda x: x['transactions'].append(copy.deepcopy(x['transactions'][0])),
                       lambda x: x['accounts'][0]['holdings'][0].update(available_quantity=200),
                       lambda x: x['accounts'][0].update(cash=None),
                       lambda x: x['transactions'][0].update(date='bad-date')]:
            bad = copy.deepcopy(p); mutate(bad)
            with self.assertRaises((ValueError, TypeError)): ops.validate_portfolio(bad)
        with self.assertRaises(ValueError): ops.validate_state(p, {}, [])
        ledger = ops.build_ledger(p, [])
        ledger['summaries'][0]['cash'] += 1
        with self.assertRaises(ValueError): ops.validate_state(p, ledger, [])

    def test_full_day_weighted_cost_and_legacy_dedup(self):
        p = fixture(); p['accounts'][0]['holdings'] = [holding(300)]
        for index, (q, price) in enumerate([(100, 10), (200, 13)]):
            p['transactions'].append({'id': str(index), 'account_id': 'a', 'date': '2026-09-01', 'kind': 'Buy',
                'code': '600000', 'quantity': q, 'price': price, 'fees': 0, 'cash_amount': -q * price})
        ops.repair_intraday(p)
        self.assertEqual(p['accounts'][0]['holdings'][0]['intraday_cost_price'], 12)
        rows = ops.deduplicate_legacy([{'date': 'x', 'quantity': 100}, {'date': 'x', 'quantity': 100}])
        self.assertEqual(len(rows), 1)

    def test_transaction_bundle_rollback_and_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ops.atomic_json(root / 'portfolio.json', fixture())
            with ops.locked(root):
                with self.assertRaises(FileExistsError):
                    with ops.locked(root): pass
            original = (root / 'portfolio.json').read_bytes()
            writer = ops.atomic_json
            def failure(path, value):
                if path.name == 'portfolio.json': raise OSError('injected disk failure')
                writer(path, value)
            with patch.object(ops, 'atomic_json', failure):
                with self.assertRaises(OSError): ops.save_bundle(root, fixture(), [])
            self.assertEqual((root / 'portfolio.json').read_bytes(), original)
            self.assertFalse((root / 'trade_ledger.json').exists())


if __name__ == '__main__':
    unittest.main()
