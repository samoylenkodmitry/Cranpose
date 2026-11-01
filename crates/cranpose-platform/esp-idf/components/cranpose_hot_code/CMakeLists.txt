# Moves the framework's per-frame code into IRAM with a linker script of the
# chip, when CONFIG_CRANPOSE_HOT_CODE_IN_IRAM is on, and puts the rest of the
# code its profiles ran first in flash, in the order of a file of the chip.
# See README.md.
idf_component_register()
set(script "${CMAKE_CURRENT_LIST_DIR}/${IDF_TARGET}.ld")
set(order "${CMAKE_CURRENT_LIST_DIR}/${IDF_TARGET}.order")
if(CONFIG_CRANPOSE_HOT_CODE_IN_IRAM AND EXISTS "${script}")
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,-T,${script}")
endif()
if(CONFIG_CRANPOSE_HOT_CODE_IN_IRAM AND EXISTS "${order}")
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-Wl,--section-ordering-file,${order}")
endif()
