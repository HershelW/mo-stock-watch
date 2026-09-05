"""Exercise the actual accounting functions without importing training dependencies."""
import ast
import json
import unittest
from pathlib import Path
import pandas as pd
import numpy as np


def functions():
    source = Path(__file__).with_name('trading_clone.py').read_text(encoding='utf-8')
    tree = ast.parse(source)
    namespace = {'pd': pd, 'np': np, 'json': json, 'CONFIDENCE_WEIGHT': {'明确成交': 1.0}, 'HIGH_CONFIDENCE': {'明确成交'}}
    selected = ['curve_metrics', 'event_study', 'clean_ledger']
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name in selected:
            exec('from __future__ import annotations\n' + ast.get_source_segment(source, node), namespace)
    return namespace


class BacktestIntegrityTests(unittest.TestCase):
    def test_only_structured_execution_can_train_and_nested_evidence_deduplicates(self):
        row = {'id': 'x', 'date': '2026-07-01', 'code': '600000', 'name': 'Fixture', 'quantity': 100,
               'price': 10, 'amount': 1000, 'action': '买入', 'confidence': '明确成交', 'suffix': '1234',
               'execution': {'confidence': 'Confirmed', 'price_basis': 'fill', 'date_is_estimated': False}}
        clean = functions()['clean_ledger']
        ledger, _, _ = clean([row, row])
        self.assertEqual(len(ledger), 1)
        self.assertEqual(ledger.attrs['exact_duplicate_rows'], 1)
        with self.assertRaises(ValueError): clean([{**row, 'execution': {}, 'confidence': '用户明确说明'}])

    def test_initial_loss_counts_in_drawdown(self):
        curve = pd.DataFrame({'open_nav': [100, 80], 'nav': [80, 90]})
        metrics = functions()['curve_metrics'](curve, pd.DataFrame())
        self.assertAlmostEqual(metrics['max_drawdown'], -0.2)

    def test_actual_action_does_not_earn_predecision_day_return(self):
        dates = pd.to_datetime(['2026-07-01', '2026-07-02'])
        prices = pd.DataFrame({'date': dates, 'code': ['x', 'x'], 'open': [10, 20], 'close': [20, 18]})
        signals = pd.DataFrame({'date': [dates[0]], 'code': ['x'], 'direction': [1], 'source': ['actual']})
        output = functions()['event_study'](signals, prices, (1,))
        self.assertAlmostEqual(output.iloc[0].average_return, -0.1)


if __name__ == '__main__':
    unittest.main()
