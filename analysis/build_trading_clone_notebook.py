"""Generate a private, reproducible notebook from reviewed analysis outputs."""
from __future__ import annotations

import argparse
from pathlib import Path
import nbformat as nbf


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.input_dir.resolve()
    for filename in ("model_summary.json", "backtest_metrics.csv"):
        if not (root / filename).is_file():
            raise FileNotFoundError(root / filename)
    cells = [
        nbf.v4.new_markdown_cell("## tl;dr\n\nExploratory analysis. Historical test windows have already been inspected; results do not establish an independent out-of-sample edge."),
        nbf.v4.new_markdown_cell("## Context & Methods\n\nUse the saved source parameters and evidence classifications. Snapshot dates are observation dates, not confirmed executions. Real action events are measured from the next trading open. Costs, instrument restrictions and corporate actions must be reviewed before interpreting simulated returns."),
        nbf.v4.new_code_cell("from pathlib import Path\nimport json\nimport pandas as pd\nroot = Path(" + repr(str(root)) + ")\nsummary = json.loads((root / 'model_summary.json').read_text(encoding='utf-8'))\nprint(json.dumps(summary, ensure_ascii=False, indent=2))"),
        nbf.v4.new_markdown_cell("## Data"),
        nbf.v4.new_code_cell("if (root / 'data_quality.json').exists():\n    print((root / 'data_quality.json').read_text(encoding='utf-8'))"),
        nbf.v4.new_markdown_cell("## Results"),
        nbf.v4.new_code_cell("metrics = pd.read_csv(root / 'backtest_metrics.csv')\nprint(metrics.to_string(index=False))"),
        nbf.v4.new_markdown_cell("## Takeaways\n\nRead metrics together with data coverage and confidence. Uncertain or incomplete executions cannot establish the trader's real performance. Do not infer short-term skill from opening prices observed before an actual decision."),
    ]
    notebook = nbf.v4.new_notebook(cells=cells, metadata={
        "kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
        "language_info": {"name": "python"},
    })
    # Execution is optional: never claim that merely loading saved results reruns a backtest.
    nbf.validate(notebook)
    target = args.output or root / "backtest_review.ipynb"
    nbf.write(notebook, target)
    print(target)


if __name__ == "__main__":
    main()
