"""移行元を編集せず、依存import後に警告設定を戻してmainを呼ぶ。"""

import os
import warnings

import owlmixin  # noqa: F401 -- import時にwarnings.simplefilter('once')を設定する
from jumeaux.main import main

if os.environ.get("PYTHONWARNINGS") == "ignore":
    warnings.simplefilter("ignore")

if __name__ == "__main__":
    main()
