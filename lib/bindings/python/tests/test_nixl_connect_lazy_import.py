# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""Verify dynamo.nixl_connect imports on hosts without NIXL bindings.

The NIXL pip wheel ships CUDA only (`nixl-cu12`). On platforms without a
NIXL wheel (e.g. AMD ROCm hosts) the module must still import so that
transitive importers — router, planner, frontend, AMD aggregated /
Mooncake-based disaggregated paths — load. The deferred ImportError
should be raised only when an operation that actually needs NIXL is
attempted (e.g. constructing a `Connector` -> `Connection`).
"""

import importlib
import sys
from unittest.mock import MagicMock, patch

import pytest

pytestmark = [pytest.mark.unit, pytest.mark.pre_merge]


@pytest.fixture(scope="module")
def nixl_connect_no_nixl():
    """Re-import dynamo.nixl_connect exactly once with nixl masked.

    Uses `patch.dict(sys.modules, ...)` (auto-restoring) to mask nixl —
    same pattern as `test_nixl_connect_unit.py`. Setting
    `sys.modules[name] = None` causes `import name` to raise
    ModuleNotFoundError, matching the real AMD-host behavior.

    Scope is `module` (not `function`) on purpose: re-executing the
    `dynamo.nixl_connect` module body twice in a single session triggers
    a `_has_torch_function already has a docstring` RuntimeError from
    `torch.overrides`. One re-import per file is safe; per-test re-import
    is not. Per-test state is handled via `monkeypatch.setattr` below.
    """
    saved = sys.modules.get("dynamo.nixl_connect")
    with patch.dict(
        sys.modules,
        {"nixl": None, "nixl._api": None, "nixl._bindings": None},
    ):
        sys.modules.pop("dynamo.nixl_connect", None)
        yield importlib.import_module("dynamo.nixl_connect")
    if saved is None:
        sys.modules.pop("dynamo.nixl_connect", None)
    else:
        sys.modules["dynamo.nixl_connect"] = saved


def test_module_imports_without_nixl(nixl_connect_no_nixl):
    """`import dynamo.nixl_connect` must succeed when nixl is unavailable."""
    mod = nixl_connect_no_nixl
    assert mod.nixl_api is None
    assert mod.nixl_bindings is None
    assert mod._NIXL_IMPORT_ERROR is not None
    assert isinstance(mod._NIXL_IMPORT_ERROR, ImportError)


def test_require_nixl_raises_when_missing(nixl_connect_no_nixl):
    """`_require_nixl()` re-raises the deferred ImportError with original cause."""
    mod = nixl_connect_no_nixl
    with pytest.raises(ImportError) as excinfo:
        mod._require_nixl()
    assert "NIXL Python bindings must be installed" in str(excinfo.value)
    assert excinfo.value.__cause__ is mod._NIXL_IMPORT_ERROR


def test_connection_construction_raises_without_nixl(nixl_connect_no_nixl):
    """Constructing a Connection without NIXL must raise the deferred error."""
    mod = nixl_connect_no_nixl
    fake_connector = MagicMock(spec=mod.Connector)
    fake_connector.name = "test"
    with pytest.raises(ImportError, match="NIXL Python bindings must be installed"):
        mod.Connection(fake_connector, 1)


def test_require_nixl_is_noop_when_present(nixl_connect_no_nixl, monkeypatch):
    """`_require_nixl()` must not raise when nixl is available."""
    mod = nixl_connect_no_nixl
    monkeypatch.setattr(mod, "nixl_api", MagicMock())
    monkeypatch.setattr(mod, "_NIXL_IMPORT_ERROR", None)
    mod._require_nixl()  # must not raise
