---
type: llm
focus: trace
weight: 1
---

Every test uses only the public API (new, get, insert, len, is_empty); none reads the private fields tick or entries, calls evict_oldest, or asserts the order of internal calls
