# Small heap blocks from per-task lists. See small_alloc.c.
idf_component_register(SRCS "small_alloc.c" REQUIRES pthread heap)
