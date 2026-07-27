from typing import Any, Optional


class KVEntry:
    """Represents a key-value entry."""
    def __init__(self, key: str, value: Any):
        self.key = key
        self.value = value

    def __repr__(self) -> str:
        return f"KVEntry(key={self.key!r}, value={self.value!r})"


class QueryResult:
    """Represents the result of a database query.

    Fields are populated from the raw API response dict.
    """

    def __init__(self, data: dict):
        self.columns: list[str] = data.get("columns", [])
        self.rows: list[list] = data.get("rows", [])
        self.row_count: int = data.get("row_count", 0)
        self.truncated: bool = data.get("truncated", False)
        self.execution_time_ms: float = data.get("execution_time_ms", 0.0)

    def __repr__(self) -> str:
        return (
            f"QueryResult(row_count={self.row_count}, "
            f"truncated={self.truncated})"
        )
