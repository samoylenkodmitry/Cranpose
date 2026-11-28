// Writes a line to the console's transmit FIFO only when the FIFO can take
// the whole line now, so the frame loop never waits on the console.
//
// ESP-IDF's console write waits on a full FIFO. On the USB Serial/JTAG
// without a driver, `usb_serial_jtag_tx_char_no_driver` waits until 50 ms
// after the last byte that went in (`TX_FLUSH_TIMEOUT_US` in
// usb_serial_jtag_vfs.c, a constant with no Kconfig): a host that is
// connected and reads nothing, as `espflash monitor` while it opens the port
// after a reset, holds each line for 50 ms. On a UART, `uart_tx_char` waits
// until the UART has sent a byte: 87 us a byte at 115200 baud.
//
// The bytes go straight into the FIFO, beside ESP-IDF's own writers and
// their lock: a line of another task can interleave with one written here.

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "sdkconfig.h"

#if CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG
#include "hal/usb_serial_jtag_ll.h"

// The IN endpoint's FIFO holds one packet. It takes bytes while
// `serial_in_ep_data_free` is set; once a packet is flushed it takes none
// until the host has read it.
#define PACKET_BYTES 64

bool cranpose_console_write_line(const uint8_t *line, size_t len)
{
    if (!usb_serial_jtag_ll_txfifo_writable()) {
        return false;
    }
    // The endpoint's write and read addresses count the bytes in the FIFO.
    uint32_t used = (USB_SERIAL_JTAG.in_ep1_st.in_ep1_wr_addr - USB_SERIAL_JTAG.in_ep1_st.in_ep1_rd_addr) & 0x7f;
    if (used > PACKET_BYTES || len > PACKET_BYTES - used) {
        return false;
    }
    uint32_t written = usb_serial_jtag_ll_write_txfifo(line, len);
    usb_serial_jtag_ll_txfifo_flush();
    return written == len;
}

#elif CONFIG_ESP_CONSOLE_UART
#include "hal/uart_ll.h"

bool cranpose_console_write_line(const uint8_t *line, size_t len)
{
    uart_dev_t *uart = UART_LL_GET_HW(CONFIG_ESP_CONSOLE_UART_NUM);
    if (uart_ll_get_txfifo_len(uart) < len) {
        return false;
    }
    uart_ll_write_txfifo(uart, line, len);
    return true;
}

#else

// No console: every line is dropped.
bool cranpose_console_write_line(const uint8_t *line, size_t len)
{
    (void)line;
    (void)len;
    return false;
}

#endif
