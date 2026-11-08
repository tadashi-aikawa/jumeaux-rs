"""基準リポジトリを編集せず、承認された旧サンプル2件の補正だけを適用する。"""
import importlib
import warnings

from jumeaux.main import main
import jumeaux.addons
from jumeaux.models import JudgementAddOnPayload
from jumeaux.utils import get_by_diff_key

ignore = importlib.import_module("addon_sample.judgement.ignore_values")
table = importlib.import_module("addon_sample.final.table")


def corrected_ignore(self, payload, reference):
    data = payload.to_dict()
    diffs = data.get("diffs_by_cognition")
    if not self.config.values or payload.regard_as_same_body or not diffs or "unknown" not in diffs:
        return payload
    for kind in ("added", "changed", "removed"):
        kept, ignored = [], []
        for key in diffs["unknown"][kind]:
            one = get_by_diff_key(reference.dict_one.get(), key)
            other = get_by_diff_key(reference.dict_other.get(), key)
            target = ignored if any(value in str(one) or value in str(other) for value in self.config.values) else kept
            target.append(key)
        diffs["unknown"][kind] = kept
        if ignored:
            diffs.setdefault("ignore_values", {"added": [], "changed": [], "removed": []})[kind].extend(ignored)
    if not any(diffs["unknown"].values()):
        del diffs["unknown"]
        data["regard_as_same_body"] = True
    return JudgementAddOnPayload.from_dict(data)


old_table = table.Executor.exec
ignore.Executor.exec = corrected_ignore
table.Executor.exec = lambda self, payload, reference: old_table(self, payload)
warnings.simplefilter("ignore")
if __name__ == "__main__":
    main()
