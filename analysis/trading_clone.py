from __future__ import annotations

import argparse
import json
import math
import os
import sqlite3
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlencode

import numpy as np
import pandas as pd
import requests
import joblib
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import precision_recall_fscore_support
from sklearn.compose import ColumnTransformer
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import OneHotEncoder, StandardScaler


FEATURES = [
    "ret_1_lag",
    "ret_3_lag",
    "ret_5_lag",
    "ret_10_lag",
    "ret_20_lag",
    "ma5_dist_lag",
    "ma20_dist_lag",
    "drawdown_20_lag",
    "volatility_10_lag",
    "volume_ratio_5_lag",
    "market_ret_5_lag",
    "relative_strength_5_lag",
]

MODEL_COLUMNS = FEATURES + ["code"]

FEATURE_LABELS = {
    "ret_1_lag": "前一日涨跌",
    "ret_3_lag": "3日动量",
    "ret_5_lag": "5日动量",
    "ret_10_lag": "10日动量",
    "ret_20_lag": "20日动量",
    "ma5_dist_lag": "偏离5日均线",
    "ma20_dist_lag": "偏离20日均线",
    "drawdown_20_lag": "距20日高点",
    "volatility_10_lag": "10日波动率",
    "volume_ratio_5_lag": "5日量比",
    "market_ret_5_lag": "沪深300五日动量",
    "relative_strength_5_lag": "相对沪深300强度",
    "days_since_buy": "距上次买入天数",
    "days_since_sell": "距上次卖出天数",
    "buy_events_20": "20日内买入次数",
    "sell_events_20": "20日内卖出次数",
    "held_proxy": "是否处于持有状态",
    "last_action_side": "上一次操作方向",
}

CONFIDENCE_WEIGHT = {
    "明确成交": 1.0,
    "用户明确说明": 1.0,
    "明确截图": 1.0,
    "截图推断/待核": 0.70,
    "截图校准/待核": 0.50,
    "快照差分": 0.35,
    "期初快照": 0.25,
}

HIGH_CONFIDENCE = {"明确成交"}


def parse_args() -> argparse.Namespace:
    app_dir = Path(os.environ["APPDATA"]) / "mo-stock-watch"
    parser = argparse.ArgumentParser(description="Build an interpretable clone of the user's trading timing.")
    parser.add_argument("--ledger", type=Path, default=app_dir / "trade_ledger.json")
    parser.add_argument("--portfolio", type=Path, default=app_dir / "portfolio.json")
    parser.add_argument(
        "--start-snapshot",
        type=Path,
        required=True,
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--refresh-prices", action="store_true")
    parser.add_argument("--train-end", default="2026-05-31")
    parser.add_argument("--validation-end", default="2026-06-30")
    parser.add_argument("--test-end", default="2026-08-14")
    return parser.parse_args()


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def market_id(code: str) -> int:
    if code == "000300":
        return 1
    return 1 if code.startswith(("5", "6", "9")) else 0


def fetch_sina_history(code: str) -> pd.DataFrame:
    prefix = "sh" if market_id(code) == 1 else "sz"
    symbol = f"{prefix}{code}"
    url = (
        "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/"
        f"CN_MarketData.getKLineData?symbol={symbol}&scale=240&ma=no&datalen=250"
    )
    ps_script = (
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new();"
        "$ProgressPreference='SilentlyContinue';"
        f"$r=Invoke-RestMethod -Uri '{url}' "
        "-Headers @{Referer='https://finance.sina.com.cn/';'User-Agent'='Mozilla/5.0'} "
        "-TimeoutSec 20;"
        "$r | ConvertTo-Json -Compress -Depth 6"
    )
    completed = subprocess.run(
        ["powershell.exe", "-NoProfile", "-Command", ps_script],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=30,
        check=False,
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        raise RuntimeError(f"Sina history request failed for {code}: {completed.stderr.strip()}")
    payload = json.loads(completed.stdout)
    if isinstance(payload, dict):
        payload = [payload]
    frame = pd.DataFrame(payload).rename(columns={"day": "date"})
    if frame.empty:
        raise RuntimeError(f"No Sina history returned for {code}")
    frame["date"] = pd.to_datetime(frame["date"])
    for column in ["open", "close", "high", "low", "volume"]:
        frame[column] = pd.to_numeric(frame[column], errors="coerce")
    frame = frame.sort_values("date").reset_index(drop=True)
    frame["amount"] = np.nan
    frame["amplitude_pct"] = (frame["high"] / frame["low"] - 1) * 100
    frame["change"] = frame["close"].diff()
    frame["change_pct"] = frame["close"].pct_change() * 100
    frame["turnover_pct"] = np.nan
    frame["code"] = code
    frame["name"] = code
    frame["source"] = "Sina daily K-line"
    return frame


def fetch_history(code: str, output: Path, refresh: bool, beg: str = "20260101", end: str = "20260814") -> pd.DataFrame:
    output.mkdir(parents=True, exist_ok=True)
    cache_path = output / f"{code}.csv"
    if cache_path.exists() and not refresh:
        frame = pd.read_csv(cache_path, dtype={"code": str}, parse_dates=["date"])
        if frame["date"].max() >= pd.Timestamp(end):
            return frame[frame["date"] <= pd.Timestamp(end)]

    if os.name == "nt":
        frame = fetch_sina_history(code)
        frame = frame[(frame["date"] >= pd.Timestamp(beg)) & (frame["date"] <= pd.Timestamp(end))]
        frame.to_csv(cache_path, index=False, encoding="utf-8-sig")
        time.sleep(0.08)
        return frame

    url = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
    params = {
        "secid": f"{market_id(code)}.{code}",
        "klt": "101",
        "fqt": "1",
        "beg": beg,
        "end": end,
        "lmt": "500",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
    }
    session = requests.Session()
    session.trust_env = False
    response = None
    last_error: Exception | None = RuntimeError("PowerShell transport selected on Windows")
    if os.name != "nt":
        last_error = None
        for attempt in range(3):
            try:
                response = session.get(
                    url,
                    params=params,
                    headers={"Referer": "https://quote.eastmoney.com/", "User-Agent": "Mozilla/5.0"},
                    timeout=20,
                )
                response.raise_for_status()
                last_error = None
                break
            except requests.RequestException as error:
                last_error = error
                time.sleep(0.4 * (attempt + 1))
    if response is None or last_error is not None:
        full_url = f"{url}?{urlencode(params)}"
        ps_script = (
            "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new();"
            f"$r=Invoke-RestMethod -Uri '{full_url}' "
            "-Headers @{Referer='https://quote.eastmoney.com/'} -TimeoutSec 20;"
            "$r | ConvertTo-Json -Compress -Depth 8"
        )
        completed = subprocess.run(
            ["powershell.exe", "-NoProfile", "-Command", ps_script],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
            check=False,
        )
        if completed.returncode != 0 or not completed.stdout.strip():
            raise RuntimeError(
                f"History request failed for {code}: {completed.stderr.strip()}"
            ) from last_error
        payload = json.loads(completed.stdout)
    else:
        payload = response.json()
    if payload.get("rc") != 0 or not payload.get("data") or not payload["data"].get("klines"):
        raise RuntimeError(f"No history returned for {code}: {payload.get('rc')}")
    rows = [item.split(",") for item in payload["data"]["klines"]]
    columns = [
        "date",
        "open",
        "close",
        "high",
        "low",
        "volume",
        "amount",
        "amplitude_pct",
        "change_pct",
        "change",
        "turnover_pct",
    ]
    frame = pd.DataFrame(rows, columns=columns)
    frame["date"] = pd.to_datetime(frame["date"])
    for column in columns[1:]:
        frame[column] = pd.to_numeric(frame[column], errors="coerce")
    frame["code"] = code
    frame["name"] = payload["data"].get("name", code)
    frame.to_csv(cache_path, index=False, encoding="utf-8-sig")
    time.sleep(0.08)
    return frame


def clean_ledger(raw_rows: list[dict[str, Any]]) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    eligible = [r for r in raw_rows if r.get("is_execution", True)
                and r.get("execution", {}).get("confidence") == "Confirmed"
                and r.get("execution", {}).get("price_basis") == "fill"
                and r.get("execution", {}).get("date_is_estimated") is False
                and r.get("action") in {"买入", "卖出"}]
    unique = {json.dumps(r, sort_keys=True, ensure_ascii=False): r for r in eligible}
    ledger = pd.DataFrame(list(unique.values())).copy()
    if ledger.empty:
        raise ValueError("No dated execution evidence: snapshot-only records cannot train a timing model")
    exact_duplicate_rows = len(eligible) - len(unique)
    ledger["date"] = pd.to_datetime(ledger["date"])
    ledger["code"] = ledger["code"].astype(str).str.zfill(6)
    ledger["quantity"] = pd.to_numeric(ledger.get("quantity"), errors="coerce").fillna(0.0)
    ledger["price"] = pd.to_numeric(ledger.get("price"), errors="coerce").fillna(0.0)
    ledger["amount"] = pd.to_numeric(ledger.get("amount"), errors="coerce")
    ledger["amount"] = ledger["amount"].fillna(ledger["quantity"] * ledger["price"]).abs()
    ledger["before_qty"] = pd.to_numeric(ledger.get("before_qty"), errors="coerce")
    ledger["after_qty"] = pd.to_numeric(ledger.get("after_qty"), errors="coerce")
    ledger["side"] = np.select(
        [ledger["action"].str.startswith("买入"), ledger["action"].str.startswith("卖出")],
        [1, -1],
        default=0,
    )
    ledger["quality_weight"] = ledger["confidence"].map(CONFIDENCE_WEIGHT).fillna(0.25)
    ledger["is_high_confidence"] = ledger["confidence"].isin(HIGH_CONFIDENCE)
    ledger["suffix"] = ledger.get("suffix", "").fillna("").astype(str)
    ledger.attrs["exact_duplicate_rows"] = exact_duplicate_rows

    trades = ledger[ledger["side"] != 0].copy()
    kept_parts: list[pd.DataFrame] = []
    for _, group in trades.groupby(["date", "code", "side"], sort=True):
        if group["is_high_confidence"].any():
            group = group[group["is_high_confidence"]]
        kept_parts.append(group)
    dedup_rows = pd.concat(kept_parts, ignore_index=True) if kept_parts else trades.iloc[0:0].copy()

    daily_side = (
        dedup_rows.groupby(["date", "code", "name", "side"], as_index=False)
        .agg(
            quantity=("quantity", "sum"),
            amount=("amount", "sum"),
            quality_weight=("quality_weight", "max"),
            confidence=("confidence", lambda values: ",".join(sorted(set(map(str, values))))),
            row_count=("side", "size"),
        )
    )
    side_counts = daily_side.groupby(["date", "code"])["side"].nunique().rename("side_count")
    daily_side = daily_side.merge(side_counts, on=["date", "code"], how="left")

    labels = []
    for (date, code), group in daily_side.groupby(["date", "code"], sort=True):
        sides = set(group["side"].astype(int))
        conflict = len(sides) > 1
        direction = 0 if conflict else int(next(iter(sides)))
        labels.append(
            {
                "date": date,
                "code": code,
                "label": direction,
                "conflict": conflict,
                "quality_weight": float(group["quality_weight"].max()),
                "event_amount": float(group["amount"].sum()),
                "event_quantity": float(group["quantity"].sum()),
            }
        )
    daily_labels = pd.DataFrame(labels)
    return ledger, daily_side, daily_labels


def add_market_features(prices: pd.DataFrame, benchmark: pd.DataFrame) -> pd.DataFrame:
    pieces: list[pd.DataFrame] = []
    for code, group in prices.groupby("code", sort=False):
        group = group.sort_values("date").copy()
        close = group["close"]
        group["ret_1"] = close.pct_change(1)
        for window in (3, 5, 10, 20):
            group[f"ret_{window}"] = close.pct_change(window)
        group["ma5_dist"] = close / close.rolling(5).mean() - 1
        group["ma20_dist"] = close / close.rolling(20).mean() - 1
        group["drawdown_20"] = close / close.rolling(20).max() - 1
        group["volatility_10"] = group["ret_1"].rolling(10).std()
        group["volume_ratio_5"] = group["volume"] / group["volume"].rolling(20).mean()
        for column in [
            "ret_1",
            "ret_3",
            "ret_5",
            "ret_10",
            "ret_20",
            "ma5_dist",
            "ma20_dist",
            "drawdown_20",
            "volatility_10",
            "volume_ratio_5",
        ]:
            group[f"{column}_lag"] = group[column].shift(1)
        pieces.append(group)
    panel = pd.concat(pieces, ignore_index=True)

    benchmark = benchmark.sort_values("date").copy()
    benchmark["market_ret_5_lag"] = benchmark["close"].pct_change(5).shift(1)
    panel = panel.merge(benchmark[["date", "market_ret_5_lag"]], on="date", how="left")
    panel["relative_strength_5_lag"] = panel["ret_5_lag"] - panel["market_ret_5_lag"]
    return panel


def add_action_history(panel: pd.DataFrame, daily_labels: pd.DataFrame) -> pd.DataFrame:
    lookup = {
        (row.date, row.code): int(row.label)
        for row in daily_labels.itertuples()
        if not bool(row.conflict)
    }
    output: list[pd.DataFrame] = []
    for code, group in panel.groupby("code", sort=False):
        group = group.sort_values("date").copy()
        last_buy: pd.Timestamp | None = None
        last_sell: pd.Timestamp | None = None
        buy_dates: list[pd.Timestamp] = []
        sell_dates: list[pd.Timestamp] = []
        days_since_buy = []
        days_since_sell = []
        buy_events_20 = []
        sell_events_20 = []
        held_proxy = []
        last_action_side = []
        held_state = 0
        previous_action = 0
        for date in group["date"]:
            days_since_buy.append(min((date - last_buy).days, 60) if last_buy is not None else 60)
            days_since_sell.append(min((date - last_sell).days, 60) if last_sell is not None else 60)
            cutoff = date - pd.Timedelta(days=30)
            buy_events_20.append(sum(item >= cutoff for item in buy_dates))
            sell_events_20.append(sum(item >= cutoff for item in sell_dates))
            held_proxy.append(held_state)
            last_action_side.append(previous_action)
            action = lookup.get((date, code), 0)
            if action == 1:
                last_buy = date
                buy_dates.append(date)
                held_state = 1
                previous_action = 1
            elif action == -1:
                last_sell = date
                sell_dates.append(date)
                held_state = 0
                previous_action = -1
        group["days_since_buy"] = days_since_buy
        group["days_since_sell"] = days_since_sell
        group["buy_events_20"] = buy_events_20
        group["sell_events_20"] = sell_events_20
        group["held_proxy"] = held_proxy
        group["last_action_side"] = last_action_side
        output.append(group)
    return pd.concat(output, ignore_index=True)


def make_panel(
    prices: pd.DataFrame,
    benchmark: pd.DataFrame,
    daily_labels: pd.DataFrame,
    start_date: pd.Timestamp,
    end_date: pd.Timestamp,
) -> pd.DataFrame:
    panel = add_market_features(prices, benchmark)
    panel = add_action_history(panel, daily_labels)
    label_fields = daily_labels[["date", "code", "label", "conflict", "quality_weight"]]
    panel = panel.merge(label_fields, on=["date", "code"], how="left")
    panel["label"] = panel["label"].fillna(0).astype(int)
    panel["conflict"] = np.where(panel["conflict"].isna(), False, panel["conflict"]).astype(bool)
    panel["quality_weight"] = panel["quality_weight"].fillna(0.20)
    panel = panel[(panel["date"] >= start_date) & (panel["date"] <= end_date)].copy()
    panel = panel.dropna(subset=FEATURES + ["open", "close"])
    return panel.sort_values(["date", "code"]).reset_index(drop=True)


def fit_binary_model(
    panel: pd.DataFrame,
    positive_label: int,
    train_end: pd.Timestamp,
    valid_start: pd.Timestamp,
    valid_end: pd.Timestamp,
) -> tuple[Pipeline, float, float, pd.DataFrame]:
    train = panel[(panel["date"] <= train_end) & (~panel["conflict"])].copy()
    valid = panel[(panel["date"] >= valid_start) & (panel["date"] <= valid_end) & (~panel["conflict"])].copy()
    y_train = (train["label"] == positive_label).astype(int)
    y_valid = (valid["label"] == positive_label).astype(int)

    rows = []
    best: tuple[float, float, float] | None = None
    for c_value in (0.03, 0.10, 0.30, 1.0, 3.0):
        model = Pipeline(
            [
                (
                    "preprocess",
                    ColumnTransformer(
                        [
                            ("numeric", StandardScaler(), FEATURES),
                            ("code", OneHotEncoder(handle_unknown="ignore"), ["code"]),
                        ]
                    ),
                ),
                (
                    "model",
                    LogisticRegression(
                        C=c_value,
                        class_weight="balanced",
                        max_iter=4000,
                        random_state=42,
                    ),
                ),
            ]
        )
        model.fit(train[MODEL_COLUMNS], y_train, model__sample_weight=train["quality_weight"])
        probabilities = model.predict_proba(valid[MODEL_COLUMNS])[:, 1]
        actual_rate = max(float(y_valid.mean()), 1e-9)
        threshold = float(np.quantile(probabilities, 1 - actual_rate))
        predicted = probabilities >= threshold
        precision, recall, f1, _ = precision_recall_fscore_support(
            y_valid,
            predicted,
            average="binary",
            zero_division=0,
        )
        predicted_rate = float(predicted.mean())
        score = float(f1)
        rows.append(
            {
                "side": "buy" if positive_label == 1 else "sell",
                "C": c_value,
                "threshold": round(float(threshold), 6),
                "precision": float(precision),
                "recall": float(recall),
                "f1": float(f1),
                "predicted_rate": predicted_rate,
                "actual_rate": actual_rate,
                "selection_score": score,
            }
        )
        candidate = (score, -c_value)
        if best is None or candidate > best:
            best = candidate
            best_c = c_value
            best_threshold = threshold
            best_model = model

    # Keep the model whose probabilities were calibrated on validation data.
    return best_model, best_threshold, best_c, pd.DataFrame(rows)


def binary_metrics(actual: pd.Series, predicted: pd.Series) -> dict[str, float]:
    precision, recall, f1, _ = precision_recall_fscore_support(
        actual.astype(int), predicted.astype(int), average="binary", zero_division=0
    )
    return {"precision": float(precision), "recall": float(recall), "f1": float(f1)}


def model_coefficients(model: Pipeline, side: str) -> pd.DataFrame:
    coefs = model.named_steps["model"].coef_[0]
    names = model.named_steps["preprocess"].get_feature_names_out()
    frame = pd.DataFrame({"raw_feature": names, "coefficient": coefs})
    frame = frame[frame["raw_feature"].str.startswith("numeric__")].copy()
    frame["feature"] = frame["raw_feature"].str.replace("numeric__", "", regex=False)
    frame["factor"] = frame["feature"].map(FEATURE_LABELS)
    frame["side"] = side
    frame["absolute"] = frame["coefficient"].abs()
    frame["importance_pct"] = frame["absolute"] / frame["absolute"].sum() * 100
    frame["effect"] = np.where(frame["coefficient"] >= 0, "促进", "抑制")
    return frame.sort_values("absolute", ascending=False).reset_index(drop=True)


def load_starting_portfolio(path: Path) -> tuple[float, dict[str, float]]:
    payload = read_json(path)
    cash = 0.0
    positions: dict[str, float] = {}
    for account in payload.get("accounts", []):
        cash += float(account.get("cash", 0.0) or 0.0)
        for holding in account.get("holdings", []):
            code = str(holding["code"]).zfill(6)
            positions[code] = positions.get(code, 0.0) + float(holding.get("quantity", 0.0) or 0.0)
    return cash, positions


def portfolio_value(cash: float, positions: dict[str, float], price_map: dict[str, float]) -> float:
    if any(q > 0 and (code not in price_map or not np.isfinite(price_map[code])) for code, q in positions.items()):
        raise ValueError("Missing valuation price for held security")
    return cash + sum(quantity * price_map.get(code, 0.0) for code, quantity in positions.items())


@dataclass
class BacktestResult:
    curve: pd.DataFrame
    trades: pd.DataFrame
    final_positions: dict[str, float]


def simulate_clone(
    panel_test: pd.DataFrame,
    starting_cash: float,
    starting_positions: dict[str, float],
    buy_threshold: float,
    sell_threshold: float,
    buy_fraction_nav: float,
    sell_fraction: float,
    max_position_weight: float = 0.35,
    transaction_cost: float = 0.001,
) -> BacktestResult:
    cash = float(starting_cash)
    positions = dict(starting_positions)
    curve_rows: list[dict[str, Any]] = []
    trade_rows: list[dict[str, Any]] = []

    for date, day in panel_test.groupby("date", sort=True):
        opens = dict(zip(day["code"], day["open"]))
        closes = dict(zip(day["code"], day["close"]))
        open_nav = portfolio_value(cash, positions, opens)

        sells = day[day["predicted_sell"]].sort_values("sell_probability", ascending=False)
        for row in sells.itertuples():
            held = positions.get(row.code, 0.0)
            if held <= 0 or row.open <= 0:
                continue
            if sell_fraction >= 0.85:
                quantity = held
            else:
                quantity = math.floor(held * sell_fraction / 100.0) * 100.0
                if quantity < 100 and held >= 100:
                    quantity = 100.0
                quantity = min(quantity, held)
            if quantity <= 0:
                continue
            gross = quantity * row.open
            fee = gross * transaction_cost
            cash += gross - fee
            positions[row.code] = held - quantity
            trade_rows.append(
                {
                    "date": date,
                    "code": row.code,
                    "side": "sell",
                    "quantity": quantity,
                    "price": row.open,
                    "gross": gross,
                    "fee": fee,
                    "probability": row.sell_probability,
                }
            )

        buys = day[day["predicted_buy"]].copy()
        buys["margin"] = buys["buy_probability"] - buy_threshold
        buys = buys.sort_values("margin", ascending=False).head(3)
        for row in buys.itertuples():
            if row.open <= 0 or cash <= 0:
                continue
            nav_now = portfolio_value(cash, positions, opens)
            current_value = positions.get(row.code, 0.0) * row.open
            room = max(0.0, max_position_weight * nav_now - current_value)
            desired = min(buy_fraction_nav * nav_now, room, cash / (1 + transaction_cost))
            lot = 100.0
            quantity = math.floor(desired / row.open / lot) * lot
            if quantity <= 0:
                continue
            gross = quantity * row.open
            fee = gross * transaction_cost
            if gross + fee > cash:
                continue
            cash -= gross + fee
            positions[row.code] = positions.get(row.code, 0.0) + quantity
            trade_rows.append(
                {
                    "date": date,
                    "code": row.code,
                    "side": "buy",
                    "quantity": quantity,
                    "price": row.open,
                    "gross": gross,
                    "fee": fee,
                    "probability": row.buy_probability,
                }
            )

        close_nav = portfolio_value(cash, positions, closes)
        curve_rows.append({"date": date, "nav": close_nav, "cash": cash, "open_nav": open_nav})

    return BacktestResult(pd.DataFrame(curve_rows), pd.DataFrame(trade_rows), positions)


def simulate_actual_replay(
    panel_test: pd.DataFrame,
    daily_side: pd.DataFrame,
    starting_cash: float,
    starting_positions: dict[str, float],
    transaction_cost: float = 0.001,
) -> BacktestResult:
    cash = float(starting_cash)
    positions = dict(starting_positions)
    curve_rows: list[dict[str, Any]] = []
    trade_rows: list[dict[str, Any]] = []
    actions = daily_side.copy()
    # Observed actions may be intraday: earliest daily-bar executable replay is next open.
    calendar = sorted(panel_test["date"].unique())
    actions = actions[(actions["date"] >= calendar[0]) & (actions["date"] <= calendar[-1])].copy()
    actions["date"] = actions["date"].map(lambda d: next((x for x in calendar if x > d), pd.NaT))
    actions = actions.dropna(subset=["date"])

    for date, day in panel_test.groupby("date", sort=True):
        opens = dict(zip(day["code"], day["open"]))
        closes = dict(zip(day["code"], day["close"]))
        open_nav = portfolio_value(cash, positions, opens)
        day_actions = actions[actions["date"] == date].sort_values("side")
        for row in day_actions.itertuples():
            price = opens.get(row.code)
            if price is None or price <= 0:
                continue
            requested = float(row.quantity)
            if row.side < 0:
                quantity = min(requested, positions.get(row.code, 0.0))
                if quantity <= 0:
                    continue
                gross = quantity * price
                fee = gross * transaction_cost
                cash += gross - fee
                positions[row.code] = positions.get(row.code, 0.0) - quantity
                side = "sell"
            else:
                affordable = math.floor((cash / (1 + transaction_cost)) / price / 100.0) * 100.0
                quantity = min(requested, affordable)
                if quantity <= 0:
                    continue
                gross = quantity * price
                fee = gross * transaction_cost
                cash -= gross + fee
                positions[row.code] = positions.get(row.code, 0.0) + quantity
                side = "buy"
            trade_rows.append(
                {
                    "date": date,
                    "code": row.code,
                    "side": side,
                    "quantity": quantity,
                    "price": price,
                    "gross": gross,
                    "fee": fee,
                    "probability": np.nan,
                }
            )
        curve_rows.append(
            {
                "date": date,
                "nav": portfolio_value(cash, positions, closes),
                "cash": cash,
                "open_nav": open_nav,
            }
        )
    return BacktestResult(pd.DataFrame(curve_rows), pd.DataFrame(trade_rows), positions)


def simulate_do_nothing(
    panel_test: pd.DataFrame, starting_cash: float, starting_positions: dict[str, float]
) -> BacktestResult:
    rows = []
    for date, day in panel_test.groupby("date", sort=True):
        closes = dict(zip(day["code"], day["close"]))
        opens = dict(zip(day["code"], day["open"]))
        rows.append(
            {
                "date": date,
                "nav": portfolio_value(starting_cash, starting_positions, closes),
                "cash": starting_cash,
                "open_nav": portfolio_value(starting_cash, starting_positions, opens),
            }
        )
    return BacktestResult(pd.DataFrame(rows), pd.DataFrame(), dict(starting_positions))


def curve_metrics(curve: pd.DataFrame, trades: pd.DataFrame) -> dict[str, float]:
    if curve.empty:
        return {}
    start_nav = float(curve.iloc[0]["open_nav"])
    end_nav = float(curve.iloc[-1]["nav"])
    nav = pd.Series([start_nav] + curve["nav"].tolist())
    drawdown = nav / nav.cummax() - 1
    daily_returns = nav.pct_change().dropna()
    return {
        "start_nav": start_nav,
        "end_nav": end_nav,
        "total_return": end_nav / start_nav - 1,
        "max_drawdown": float(drawdown.min()),
        "daily_volatility": float(daily_returns.std()) if len(daily_returns) else 0.0,
        "trade_count": int(len(trades)),
        "turnover": float(trades["gross"].sum() / start_nav) if not trades.empty else 0.0,
        "fees": float(trades["fee"].sum()) if not trades.empty else 0.0,
    }


def event_study(signals: pd.DataFrame, price_panel: pd.DataFrame, horizons: tuple[int, ...] = (1, 3, 5, 10)) -> pd.DataFrame:
    price_groups = {code: group.sort_values("date").reset_index(drop=True) for code, group in price_panel.groupby("code")}
    rows = []
    for signal in signals.itertuples():
        group = price_groups.get(signal.code)
        if group is None:
            continue
        matches = group.index[group["date"] == signal.date]
        if len(matches) == 0:
            continue
        # Signals from real actions become known during the day, not at its opening.
        index = int(matches[0]) + 1
        if index >= len(group):
            continue
        entry = float(group.loc[index, "open"])
        for horizon in horizons:
            end_index = index + horizon - 1
            if end_index >= len(group):
                continue
            future = float(group.loc[end_index, "close"])
            directional_return = (future / entry - 1) * int(signal.direction)
            rows.append(
                {
                    "source": signal.source,
                    "date": signal.date,
                    "code": signal.code,
                    "direction": int(signal.direction),
                    "horizon": horizon,
                    "directional_return": directional_return,
                }
            )
    detail = pd.DataFrame(rows)
    if detail.empty:
        return detail
    return (
        detail.groupby(["source", "horizon"], as_index=False)
        .agg(
            signals=("directional_return", "size"),
            average_return=("directional_return", "mean"),
            median_return=("directional_return", "median"),
            hit_rate=("directional_return", lambda values: float((values > 0).mean())),
        )
    )


def main() -> None:
    args = parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    price_cache = args.output / "prices"

    ledger_payload = read_json(args.ledger)
    portfolio_payload = read_json(args.portfolio)
    ledger, daily_side, daily_labels = clean_ledger(ledger_payload["rows"])
    train_end = pd.Timestamp(args.train_end)
    valid_start = train_end + pd.Timedelta(days=1)
    valid_end = pd.Timestamp(args.validation_end)
    test_start = valid_end + pd.Timedelta(days=1)
    test_end = pd.Timestamp(args.test_end)
    if not train_end < valid_end < test_end:
        raise ValueError("Training, validation and test windows must not overlap")
    universe = sorted(ledger.loc[(ledger["side"] != 0) & (ledger["date"] <= valid_end), "code"].unique())
    name_map = ledger.groupby("code")["name"].last().to_dict()

    price_frames = []
    for code in universe:
        price_frames.append(fetch_history(code, price_cache, args.refresh_prices, end=test_end.strftime("%Y%m%d")))
    prices = pd.concat(price_frames, ignore_index=True)
    benchmark = fetch_history("000300", price_cache, args.refresh_prices, end=test_end.strftime("%Y%m%d"))

    ledger_start = ledger["date"].min()
    ledger_end = ledger["date"].max()
    panel = make_panel(prices, benchmark, daily_labels, ledger_start, ledger_end)


    buy_model, buy_threshold, buy_c, buy_grid = fit_binary_model(
        panel, 1, train_end, valid_start, valid_end
    )
    sell_model, sell_threshold, sell_c, sell_grid = fit_binary_model(
        panel, -1, train_end, valid_start, valid_end
    )

    test = panel[(panel["date"] >= test_start) & (panel["date"] <= test_end)].copy()
    test["buy_probability"] = buy_model.predict_proba(test[MODEL_COLUMNS])[:, 1]
    test["sell_probability"] = sell_model.predict_proba(test[MODEL_COLUMNS])[:, 1]
    test["predicted_buy"] = test["buy_probability"] >= buy_threshold
    test["predicted_sell"] = test["sell_probability"] >= sell_threshold
    both = test["predicted_buy"] & test["predicted_sell"]
    buy_margin = test["buy_probability"] - buy_threshold
    sell_margin = test["sell_probability"] - sell_threshold
    test.loc[both & (buy_margin >= sell_margin), "predicted_sell"] = False
    test.loc[both & (buy_margin < sell_margin), "predicted_buy"] = False

    clean_test = test[~test["conflict"]].copy()
    buy_metrics = binary_metrics(clean_test["label"] == 1, clean_test["predicted_buy"])
    sell_metrics = binary_metrics(clean_test["label"] == -1, clean_test["predicted_sell"])
    clean_test["predicted_direction"] = np.select(
        [clean_test["predicted_buy"], clean_test["predicted_sell"]], [1, -1], default=0
    )
    actual_action_rows = clean_test[clean_test["label"] != 0]
    exact_direction_match = float(
        (actual_action_rows["label"] == actual_action_rows["predicted_direction"]).mean()
    ) if len(actual_action_rows) else 0.0

    actual_keys = {
        (row.date, row.code, int(row.label))
        for row in actual_action_rows.itertuples()
    }
    predicted_rows = clean_test[clean_test["predicted_direction"] != 0]
    calendar_by_code = {
        code: list(group.sort_values("date")["date"])
        for code, group in clean_test.groupby("code")
    }
    near_hits = 0
    for row in actual_action_rows.itertuples():
        dates = calendar_by_code[row.code]
        idx = dates.index(row.date)
        nearby = set(dates[max(0, idx - 1): min(len(dates), idx + 2)])
        matched = predicted_rows[
            (predicted_rows["code"] == row.code)
            & (predicted_rows["date"].isin(nearby))
            & (predicted_rows["predicted_direction"] == row.label)
        ]
        near_hits += int(not matched.empty)
    near_direction_match = near_hits / len(actual_action_rows) if len(actual_action_rows) else 0.0

    buy_factors = model_coefficients(buy_model, "buy")
    sell_factors = model_coefficients(sell_model, "sell")
    factors = pd.concat([buy_factors, sell_factors], ignore_index=True)

    starting_cash, starting_positions = load_starting_portfolio(args.start_snapshot)
    first_day = test[test["date"] == test["date"].min()]
    first_opens = dict(zip(first_day["code"], first_day["open"]))
    starting_nav = portfolio_value(starting_cash, starting_positions, first_opens)
    training_buys = daily_side[(daily_side["side"] == 1) & (daily_side["date"] <= valid_end)]
    median_ticket = float(training_buys["amount"].median())
    buy_fraction_nav = float(np.clip(median_ticket / starting_nav, 0.03, 0.15))
    raw_sells = ledger[
        (ledger["side"] == -1)
        & (ledger["date"] <= valid_end)
        & ledger["before_qty"].notna()
        & (ledger["before_qty"] > 0)
    ].copy()
    sell_fractions = (raw_sells["quantity"] / raw_sells["before_qty"]).clip(0, 1)
    sell_fraction = float(np.clip(sell_fractions.median() if len(sell_fractions) else 0.5, 0.25, 1.0))

    clone_result = simulate_clone(
        test,
        starting_cash,
        starting_positions,
        buy_threshold,
        sell_threshold,
        buy_fraction_nav,
        sell_fraction,
    )
    actual_result = simulate_actual_replay(test, daily_side, starting_cash, starting_positions)
    hold_result = simulate_do_nothing(test, starting_cash, starting_positions)

    benchmark_test = benchmark[(benchmark["date"] >= test_start) & (benchmark["date"] <= test_end)].copy()
    benchmark_start = float(benchmark_test.iloc[0]["open"])
    benchmark_curve = benchmark_test[["date", "close"]].rename(columns={"close": "nav"})
    benchmark_curve["nav"] = benchmark_curve["nav"] / benchmark_start * starting_nav
    benchmark_curve["open_nav"] = benchmark_curve["nav"].shift(1).fillna(starting_nav)
    benchmark_curve["cash"] = 0.0

    curves = []
    for name, result in [
        ("行为克隆", clone_result),
        ("实际操作重放", actual_result),
        ("原持仓不动", hold_result),
    ]:
        frame = result.curve.copy()
        frame["strategy"] = name
        curves.append(frame)
    benchmark_curve["strategy"] = "沪深300"
    curves.append(benchmark_curve)
    equity_curves = pd.concat(curves, ignore_index=True)
    curve_start_nav = equity_curves.groupby("strategy")["open_nav"].transform("first")
    equity_curves["cumulative_return"] = equity_curves["nav"] / curve_start_nav - 1
    equity_curves["indexed_nav"] = 1 + equity_curves["cumulative_return"]

    metrics_rows = []
    for name, result in [
        ("行为克隆", clone_result),
        ("实际操作重放", actual_result),
        ("原持仓不动", hold_result),
    ]:
        metrics_rows.append({"strategy": name, **curve_metrics(result.curve, result.trades)})
    metrics_rows.append({"strategy": "沪深300", **curve_metrics(benchmark_curve, pd.DataFrame())})
    backtest_metrics = pd.DataFrame(metrics_rows)

    actual_signals = actual_action_rows[["date", "code", "label"]].rename(columns={"label": "direction"})
    actual_signals["source"] = "实际决策"
    clone_signals = predicted_rows[["date", "code", "predicted_direction"]].rename(
        columns={"predicted_direction": "direction"}
    )
    clone_signals["source"] = "行为克隆"
    signal_study = event_study(pd.concat([actual_signals, clone_signals], ignore_index=True), prices)

    current_cash = sum(float(account.get("cash", 0) or 0) for account in portfolio_payload.get("accounts", []))
    data_quality = {
        "raw_rows": int(len(ledger_payload["rows"])),
        "exact_duplicate_rows": int(ledger.attrs.get("exact_duplicate_rows", 0)),
        "rows_after_exact_deduplication": int(len(ledger)),
        "trade_rows": int((ledger["side"] != 0).sum()),
        "deduped_daily_side_events": int(len(daily_side)),
        "daily_stock_events": int(len(daily_labels)),
        "conflicting_daily_stock_events": int(daily_labels["conflict"].sum()),
        "high_confidence_trade_rows": int(ledger["is_high_confidence"].sum()),
        "snapshot_difference_trade_rows": int((ledger["confidence"] == "快照差分").sum()),
        "universe_size": int(len(universe)),
        "ledger_start": ledger_start.strftime("%Y-%m-%d"),
        "ledger_end": ledger_end.strftime("%Y-%m-%d"),
        "test_start": test_start.strftime("%Y-%m-%d"),
        "test_end": test_end.strftime("%Y-%m-%d"),
        "test_trading_days": int(test["date"].nunique()),
        "current_cash_all_accounts": current_cash,
    }

    model_summary = {
        "evaluation_status": "exploratory_reused_test_window_not_independent",
        "limitations": ["Historical traded universe only; not a stock-selection test", "Daily bars cannot prove intraday fillability", "Raw daily prices require corporate-action review", "Real action event study starts at next open"],
        "buy": {"threshold": buy_threshold, "C": buy_c, **buy_metrics},
        "sell": {"threshold": sell_threshold, "C": sell_c, **sell_metrics},
        "exact_action_direction_match": exact_direction_match,
        "plus_minus_one_day_direction_match": near_direction_match,
        "test_actual_action_events": int(len(actual_action_rows)),
        "test_predicted_action_events": int(len(predicted_rows)),
        "buy_fraction_nav": buy_fraction_nav,
        "sell_fraction": sell_fraction,
        "starting_nav": starting_nav,
        "start_snapshot": args.start_snapshot.name,
        "fixed_universe": universe,
        "universe_names": {code: name_map.get(code, code) for code in universe},
    }

    joblib.dump(
        {
            "version": "v0",
            "trained_through": train_end.strftime("%Y-%m-%d"),
            "features": FEATURES,
            "model_columns": MODEL_COLUMNS,
            "universe": universe,
            "buy_model": buy_model,
            "sell_model": sell_model,
            "buy_threshold": buy_threshold,
            "sell_threshold": sell_threshold,
            "buy_fraction_nav": buy_fraction_nav,
            "sell_fraction": sell_fraction,
            "max_position_weight": 0.35,
            "transaction_cost": 0.001,
            "status": "research_only_not_for_automatic_trading",
        },
        args.output / "behavior_clone_v0.joblib",
    )

    validation_grid = pd.concat([buy_grid, sell_grid], ignore_index=True)
    outputs = {
        "data_quality.json": data_quality,
        "model_summary.json": model_summary,
    }
    for filename, payload in outputs.items():
        with (args.output / filename).open("w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)

    ledger.to_csv(args.output / "ledger_profile.csv", index=False, encoding="utf-8-sig")
    daily_side.to_csv(args.output / "daily_side_events.csv", index=False, encoding="utf-8-sig")
    panel.to_csv(args.output / "model_panel.csv", index=False, encoding="utf-8-sig")
    test.to_csv(args.output / "test_predictions.csv", index=False, encoding="utf-8-sig")
    factors.to_csv(args.output / "factor_coefficients.csv", index=False, encoding="utf-8-sig")
    validation_grid.to_csv(args.output / "validation_grid.csv", index=False, encoding="utf-8-sig")
    equity_curves.to_csv(args.output / "equity_curves.csv", index=False, encoding="utf-8-sig")
    backtest_metrics.to_csv(args.output / "backtest_metrics.csv", index=False, encoding="utf-8-sig")
    signal_study.to_csv(args.output / "signal_event_study.csv", index=False, encoding="utf-8-sig")
    clone_result.trades.to_csv(args.output / "clone_trades.csv", index=False, encoding="utf-8-sig")
    actual_result.trades.to_csv(args.output / "actual_replay_trades.csv", index=False, encoding="utf-8-sig")

    model_validation = pd.DataFrame(
        [
            {
                "metric": "同日方向命中率",
                "value": exact_direction_match,
                "sample_size": int(len(actual_action_rows)),
            },
            {
                "metric": "正负一交易日方向命中率",
                "value": near_direction_match,
                "sample_size": int(len(actual_action_rows)),
            },
            {"metric": "买入精确率", "value": buy_metrics["precision"], "sample_size": int(len(actual_action_rows))},
            {"metric": "买入召回率", "value": buy_metrics["recall"], "sample_size": int(len(actual_action_rows))},
            {"metric": "卖出精确率", "value": sell_metrics["precision"], "sample_size": int(len(actual_action_rows))},
            {"metric": "卖出召回率", "value": sell_metrics["recall"], "sample_size": int(len(actual_action_rows))},
        ]
    )
    model_validation.to_csv(args.output / "model_validation.csv", index=False, encoding="utf-8-sig")

    with sqlite3.connect(args.output / "analysis.sqlite") as connection:
        backtest_metrics.to_sql("backtest_metrics", connection, if_exists="replace", index=False)
        equity_curves.to_sql("equity_curves", connection, if_exists="replace", index=False)
        signal_study.to_sql("signal_event_study", connection, if_exists="replace", index=False)
        factors.to_sql("factor_coefficients", connection, if_exists="replace", index=False)
        model_validation.to_sql("model_validation", connection, if_exists="replace", index=False)
        pd.DataFrame([data_quality]).to_sql("data_quality", connection, if_exists="replace", index=False)

    print(json.dumps({
        "output": str(args.output),
        "data_quality": data_quality,
        "model_summary": model_summary,
        "backtest": backtest_metrics.to_dict(orient="records"),
        "signal_study": signal_study.to_dict(orient="records"),
        "top_buy_factors": buy_factors.head(6)[["factor", "coefficient", "importance_pct", "effect"]].to_dict(orient="records"),
        "top_sell_factors": sell_factors.head(6)[["factor", "coefficient", "importance_pct", "effect"]].to_dict(orient="records"),
    }, ensure_ascii=False, indent=2, default=str))


if __name__ == "__main__":
    main()
