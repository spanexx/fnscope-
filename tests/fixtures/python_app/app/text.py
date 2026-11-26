"""Text helpers for the Python adapter fixture."""

UserId = int
Alias = dict
MAX_RETRIES = 3


def normalize_text(text: str) -> str:
    """Lowercase and trim the input."""
    return text.strip().lower()


def classify(text: str) -> str:
    """Classify a piece of text after normalization."""
    # Normalize first.
    cleaned = normalize_text(text)
    if not cleaned:
        raise ValueError("EMPTY_TEXT")
    if cleaned.endswith("e"):
        return "doer"
    return "receiver"


def report(text: str) -> None:
    # Emit the summary.
    print(text)
