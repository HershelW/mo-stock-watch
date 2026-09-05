"""Private portfolio maintenance. Standard library only; no broker orders or network calls."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
import shutil
import sys
import tempfile
from contextlib import contextmanager
from datetime import date, datetime, timezone
from pathlib import Path

KINDS = {"Buy", "Sell", "Dividend", "Fee", "Deposit", "Withdraw", "Adjustment"}
CONFIDENCE = {"Unknown", "Inferred", "Confirmed", "Snapshot"}
PRICE_BASIS = {"unknown", "estimated", "broker_cost", "fill"}


def read(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"Duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(Path(path).read_text(encoding="utf-8-sig"), object_pairs_hook=pairs,
                      parse_constant=lambda s: (_ for _ in ()).throw(ValueError(s)))


def digest(value):
    def canonical(v):
        if isinstance(v, dict): return {k: canonical(x) for k, x in v.items()}
        if isinstance(v, list): return [canonical(x) for x in v]
        if isinstance(v, float) and math.isfinite(v) and v.is_integer(): return int(v)
        return v
    return hashlib.sha256(json.dumps(canonical(value), ensure_ascii=False, sort_keys=True,
                                    separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def state_hash(p):
    return digest({'accounts': p['accounts'], 'transactions': p['transactions']})


def number(value, label, minimum=None):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"Invalid number: {label}")
    if minimum is not None and value < minimum:
        raise ValueError(f"Out of range: {label}")


def check_date(value):
    if not isinstance(value, str) or date.fromisoformat(value).isoformat() != value:
        raise ValueError(f"Invalid date: {value}")


def timestamp(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("Timestamp requires timezone")
    return parsed


def evidence(tx):
    return tx.get("execution") or {"confidence": "Inferred" if "推断" in tx.get("note", "") else "Unknown",
        "price_basis": "estimated" if tx.get("price", 0) > 0 else "unknown", "fees_known": False,
        "date_is_estimated": True, "executed_at": None, "observed_at": None}


def validate_portfolio(p):
    if not isinstance(p.get("accounts"), list) or not p["accounts"] or not isinstance(p.get("transactions"), list):
        raise ValueError("accounts and transactions arrays required")
    ids, suffixes, keys, txids = set(), set(), set(), set()
    for a in p["accounts"]:
        account_id, suffix = a.get("id"), a.get("account_suffix", "")
        if not account_id or account_id in ids or (suffix and suffix in suffixes):
            raise ValueError("Empty/duplicate account ID or duplicate suffix")
        ids.add(account_id)
        suffixes.add(suffix)
        if not isinstance(a.get("name"), str) or not isinstance(a.get("holdings"), list):
            raise ValueError("Account name and holdings required")
        number(a.get("cash"), "cash")
        number(a.get("today_realized_pnl", 0), "today_realized_pnl")
        for h in a["holdings"]:
            code = h.get("code", "")
            key = (account_id, code)
            if not isinstance(code, str) or len(code) != 6 or not code.isascii() or not code.isdigit() or key in keys:
                raise ValueError("Invalid/duplicate holding code")
            keys.add(key)
            if h.get("market") not in {"Shanghai", "Shenzhen", "Beijing"}:
                raise ValueError("Invalid market")
            number(h.get("quantity"), "holding quantity", 0.000001)
            number(h.get("cost_price"), "holding cost")
            if h.get("available_quantity") is not None:
                number(h["available_quantity"], "available quantity", 0)
                if h["available_quantity"] > h["quantity"]:
                    raise ValueError("Available quantity exceeds holding")
                check_date(h.get("available_date"))
            if h.get("intraday_cost_price") is not None:
                number(h["intraday_cost_price"], "intraday cost", 0.000001)
    for tx in p["transactions"]:
        if not tx.get("id") or tx["id"] in txids or tx.get("account_id") not in ids:
            raise ValueError("Duplicate/empty transaction ID or unknown account")
        txids.add(tx["id"])
        check_date(tx.get("date"))
        if tx.get("kind") not in KINDS:
            raise ValueError("Invalid transaction kind")
        for name in ("quantity", "price", "fees", "cash_amount"):
            number(tx.get(name), name, 0 if name in {"price", "fees"} else None)
        if tx.get("realized_pnl") is not None:
            number(tx["realized_pnl"], "realized_pnl")
        if tx["kind"] in {"Buy", "Sell"}:
            code = tx.get("code", "")
            if not isinstance(code, str) or len(code) != 6 or not code.isascii() or not code.isdigit() or tx["quantity"] <= 0:
                raise ValueError("Invalid trade code or quantity")
        e = evidence(tx)
        if e.get("confidence") not in CONFIDENCE or e.get("price_basis") not in PRICE_BASIS:
            raise ValueError("Invalid evidence classification")
        if type(e.get("fees_known")) is not bool or type(e.get("date_is_estimated")) is not bool:
            raise ValueError("Evidence flags must be booleans")
        for field in ("executed_at", "observed_at"):
            if e.get(field):
                dt = timestamp(e[field])
                if field == "executed_at" and dt.date().isoformat() != tx["date"]:
                    raise ValueError("Execution date mismatch")
        if e["price_basis"] == "fill" and tx["kind"] in {"Buy", "Sell"} and tx["price"] <= 0:
            raise ValueError("Confirmed fill must have positive price")
    return {"accounts": len(ids), "holdings": len(keys), "transactions": len(txids)}


def transaction_row(tx, a):
    e = evidence(tx)
    return {"id": tx["id"], "date": tx["date"], "time": e.get("executed_at") or "",
        "owner": a.get("owner") or "我的", "broker": a["name"], "suffix": a.get("account_suffix", ""),
        "action": {"Buy": "买入", "Sell": "卖出"}.get(tx["kind"], tx["kind"]),
        "code": tx.get("code", ""), "name": tx.get("name", ""), "quantity": tx["quantity"],
        "price": None if e["price_basis"] == "unknown" else tx["price"],
        "amount": None if e["price_basis"] == "unknown" else round(tx["price"] * tx["quantity"], 4),
        "cash_amount": tx["cash_amount"] if e["confidence"] == "Confirmed" else None,
        "estimated_cash_amount": tx["cash_amount"] if e["confidence"] != "Confirmed" else None,
        "fees": tx["fees"] if e["fees_known"] else None, "realized_pnl": tx.get("realized_pnl"),
        "execution": e, "confidence": {"Confirmed": "明确成交", "Inferred": "截图推断/待核",
        "Unknown": "待核", "Snapshot": "持仓校准"}[e["confidence"]], "note": tx.get("note", "")}


def build_ledger(p, history):
    validate_portfolio(p)
    accounts = {a["id"]: a for a in p["accounts"]}
    rows = list(history)
    rows.extend(transaction_row(tx, accounts[tx["account_id"]]) for tx in p["transactions"])
    row_ids = [r["id"] for r in rows]
    if len(set(row_ids)) != len(row_ids):
        raise ValueError("Duplicate ledger row ID")
    rows.sort(key=lambda r: (r.get("date", ""), r.get("time", ""), r["id"]))
    positions = [{"account_id": a["id"], "owner": a.get("owner") or "我的", "broker": a["name"],
                  "suffix": a.get("account_suffix", ""), **h} for a in p["accounts"] for h in a["holdings"]]
    summaries = [{"account_id": a["id"], "owner": a.get("owner") or "我的", "broker": a["name"],
                  "suffix": a.get("account_suffix", ""), "cash": a["cash"], "holdings": len(a["holdings"])}
                 for a in p["accounts"]]
    return {"version": 2, "portfolio_hash": state_hash(p), "updated_at": datetime.now(timezone.utc).isoformat(),
            "history_hash": digest(history), "rows": rows, "positions": positions, "summaries": summaries}


def validate_state(p, ledger, history):
    counts = validate_portfolio(p)
    expected = build_ledger(p, history)
    for key in ("version", "portfolio_hash", "history_hash", "rows", "positions", "summaries"):
        if ledger.get(key) != expected[key]:
            raise ValueError(f"Ledger mismatch: {key}; rebuild required")
    return {**counts, "ledger_rows": len(ledger["rows"]), "structure_valid": True,
            "ledger_matches": True, "cash_snapshot_matches": True,
            "execution_reconciled": all(evidence(t)["confidence"] == "Confirmed" and
                evidence(t)["fees_known"] and not evidence(t)["date_is_estimated"] for t in p["transactions"])}


def deduplicate_legacy(rows):
    unique = {}
    for row in rows:
        key = digest(row)
        unique.setdefault(key, {**row, "id": "legacy-" + key, "legacy": True})
    return list(unique.values())


def migrate(p, old_ledger):
    p = copy.deepcopy(p)
    account_map = {a["id"]: a for a in p["accounts"]}
    for tx in p["transactions"]:
        tx["execution"] = evidence(tx)
        if tx["execution"]["confidence"] != "Confirmed":
            tx["realized_pnl"] = None
    history = []
    for row in deduplicate_legacy(old_ledger.get("rows", [])):
        matches = [tx for tx in p["transactions"] if tx["date"] == row.get("date")
            and account_map[tx["account_id"]].get("account_suffix", "") == row.get("suffix", "")
            and tx.get("code") == row.get("code") and tx["quantity"] == row.get("quantity")
            and {"Buy": "买入", "Sell": "卖出"}.get(tx["kind"], tx["kind"]) == row.get("action")
            and abs(tx["price"] - (row.get("price") or 0)) < 0.0001]
        if not matches:
            # Preserve old snapshot differences as evidence, never reinterpret them as fresh fills.
            row["is_execution"] = False
            row["source_confidence"] = row.get("confidence", "")
            row["confidence"] = "历史证据/待核"
            history.append(row)
    repair_intraday(p)
    return p, history


def repair_intraday(p):
    for a in p["accounts"]:
        for h in a["holdings"]:
            buys = [t for t in p["transactions"] if t["account_id"] == a["id"] and t.get("code") == h["code"] and t["kind"] == "Buy"]
            if not buys:
                continue
            last_day = max(t["date"] for t in buys)
            missing = h["quantity"] - h.get("available_quantity", h["quantity"])
            day_buys = [t for t in buys if t["date"] == last_day]
            day_qty = sum(t["quantity"] for t in day_buys)
            # Only repair an unambiguous full-day lot; never guess a remaining partial lot.
            if missing > 0 and abs(day_qty - missing) < 1e-6 and all(t["price"] > 0 for t in day_buys):
                h["intraday_cost_price"] = sum(t["quantity"] * t["price"] + (t["fees"] if evidence(t)["fees_known"] else 0) for t in day_buys) / day_qty
                h["available_date"] = last_day
            elif missing == 0:
                h.pop("intraday_cost_price", None)


def apply_batch(p, batch):
    if batch.get("expected_hash") != state_hash(p):
        raise ValueError("Stale input: inspect current portfolio and prepare again")
    observed = timestamp(batch["observed_at"])
    result = copy.deepcopy(p)
    txids = {t["id"] for t in result["transactions"]}
    supplied = copy.deepcopy(batch.get("transactions", []))
    seen = set()
    for update in batch["accounts"]:
        suffix = update["suffix"]
        if suffix in seen:
            raise ValueError("Duplicate account update")
        seen.add(suffix)
        matches = [a for a in result["accounts"] if a.get("account_suffix") == suffix]
        if len(matches) != 1:
            raise ValueError("Unknown or ambiguous suffix")
        a = matches[0]
        before = {h["code"]: h for h in a["holdings"]}
        incoming = copy.deepcopy(update["holdings"])
        if len({h["code"] for h in incoming}) != len(incoming):
            raise ValueError("Duplicate incoming holding")
        after = {h["code"]: h for h in incoming}
        explicit_codes = {t.get("code", "") for t in supplied if t["account_id"] == a["id"] and t["kind"] in {"Buy", "Sell"}}
        for code in sorted(before.keys() | after.keys() | explicit_codes):
            old, new = before.get(code), after.get(code)
            delta = (new or {}).get("quantity", 0) - (old or {}).get("quantity", 0)
            matching = [t for t in supplied if t["account_id"] == a["id"] and t.get("code") == code]
            if matching:
                net = sum(t["quantity"] * (1 if t["kind"] == "Buy" else -1 if t["kind"] == "Sell" else 0) for t in matching)
                if abs(net - delta) > 1e-6:
                    raise ValueError("Explicit trades do not explain position delta")
                continue
            if abs(delta) < 1e-6:
                continue
            basis, price = "unknown", 0.0
            if delta > 0:
                amount = new["quantity"] * new["cost_price"] - ((old or {}).get("quantity", 0) * (old or {}).get("cost_price", 0))
                if amount > 0:
                    price, basis = amount / delta, "estimated" if old else "broker_cost"
            e = {"confidence": "Inferred", "price_basis": basis, "fees_known": False,
                 "date_is_estimated": True, "executed_at": None, "observed_at": batch["observed_at"]}
            supplied.append({"id": "snapshot-" + digest([batch["observed_at"], suffix, code, delta])[:24],
                "account_id": a["id"], "date": observed.date().isoformat(), "kind": "Buy" if delta > 0 else "Sell",
                "code": code, "name": (new or old)["name"], "market": (new or old)["market"],
                "quantity": abs(delta), "price": price, "fees": 0.0, "cash_amount": -delta * price,
                "realized_pnl": None, "execution": e,
                "note": "截图推断/待核；观察日归档，成交时间未知。" + update.get("note", "")})
        a["cash"] = update["cash"]
        a["holdings"] = incoming
    for tx in supplied:
        if tx["id"] in txids:
            raise ValueError("Duplicate transaction ID")
        if not any(a["id"] == tx["account_id"] and a.get("account_suffix") in seen for a in result["accounts"]):
            raise ValueError("Transaction belongs to account outside batch")
        txids.add(tx["id"])
        tx.setdefault("execution", evidence(tx))
        tx.setdefault("realized_pnl", None)
        result["transactions"].append(tx)
    repair_intraday(result)
    result["last_saved_at"] = datetime.now().astimezone().isoformat()
    validate_portfolio(result)
    return result


def atomic_json(path, value):
    path = Path(path)
    content = json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    fd, name = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


@contextmanager
def locked(root):
    lock = root / "portfolio.lock"
    fd = os.open(lock, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
    os.close(fd)
    try:
        yield
    finally:
        lock.unlink()


def save_bundle(root, p, history):
    ledger = build_ledger(p, history)
    validate_state(p, ledger, history)
    backup = root / "manual-backups" / datetime.now().strftime("ops-%Y%m%d-%H%M%S-%f")
    backup.mkdir(parents=True)
    names = ("portfolio.json", "trade_ledger.json", "ledger_history.json")
    existed = {name: (root / name).exists() for name in names}
    for name in names:
        if existed[name]:
            shutil.copy2(root / name, backup / name)
    try:
        atomic_json(root / "ledger_history.json", history)
        atomic_json(root / "trade_ledger.json", ledger)
        atomic_json(root / "portfolio.json", p)
        check = validate_state(read(root / "portfolio.json"), read(root / "trade_ledger.json"), read(root / "ledger_history.json"))
    except Exception:
        for name in names:
            if existed[name]:
                shutil.copy2(backup / name, root / name)
            elif (root / name).exists():
                (root / name).unlink()
        raise
    return {**check, "backup": str(backup), "portfolio_hash": state_hash(p)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["inspect", "validate", "migrate", "update", "rebuild", "export"])
    parser.add_argument("--app-dir", type=Path, default=Path(os.environ.get("MO_STOCK_APP_DIR") or Path(os.environ.get("APPDATA", Path.home())) / "mo-stock-watch"))
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--portfolio-only", action="store_true")
    args = parser.parse_args()
    root = args.app_dir.resolve()
    p = read(root / "portfolio.json")
    if args.command == "inspect":
        print(json.dumps({**validate_portfolio(p), "portfolio_hash": state_hash(p), "state": p["accounts"]}, ensure_ascii=False))
        return
    if args.command == "validate":
        result = validate_portfolio(p) if args.portfolio_only else validate_state(p, read(root / "trade_ledger.json"), read(root / "ledger_history.json"))
    elif args.command == "export":
        import csv
        history = read(root / "ledger_history.json")
        rows = build_ledger(p, history)["rows"]
        output = args.output or root / "trade_ledger.csv"
        fields = ["id", "date", "time", "suffix", "action", "code", "name", "quantity", "price", "amount", "fees", "realized_pnl", "confidence", "note"]
        with output.open("w", encoding="utf-8-sig", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
            writer.writeheader()
            for row in rows:
                writer.writerow({key: ("'" + value if isinstance(value, str) and value.startswith(("=", "+", "-", "@")) else value) for key, value in row.items()})
        result = {"export": str(output), "rows": len(rows)}
    else:
        with locked(root):
            p = read(root / "portfolio.json")
            if args.command == "migrate":
                if (root / "ledger_history.json").exists():
                    raise ValueError("Already migrated; use rebuild")
                p, history = migrate(p, read(root / "trade_ledger.json"))
            else:
                history = read(root / "ledger_history.json")
                if args.command == "update":
                    if not args.input:
                        raise ValueError("--input required")
                    p = apply_batch(p, read(args.input))
            result = save_bundle(root, p, history)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(2)
