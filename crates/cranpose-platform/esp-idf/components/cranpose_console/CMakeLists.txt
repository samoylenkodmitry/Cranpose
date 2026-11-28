# Console lines that never wait on the console's FIFO. See console.c.
idf_component_register(SRCS "console.c" REQUIRES hal soc)
