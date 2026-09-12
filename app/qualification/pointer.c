#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <wayland-client.h>
#include "virtual-pointer.h"

static struct zwlr_virtual_pointer_manager_v1 *manager;
static struct wl_output *output;
static unsigned outputs;

static void global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version) {
    (void)data;
    if (!strcmp(interface, "zwlr_virtual_pointer_manager_v1") && version >= 2)
        manager = wl_registry_bind(registry, name, &zwlr_virtual_pointer_manager_v1_interface, 2);
    if (!strcmp(interface, "wl_output")) {
        outputs++;
        if (!output) output = wl_registry_bind(registry, name, &wl_output_interface, 1);
    }
}

static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}

int main(int argc, char **argv) {
    char host[256] = {0};
    gethostname(host, sizeof(host) - 1);
    if (argc != 4 || strcmp(host, "omarchy-test")) return 2;
    int width = atoi(argv[2]), height = atoi(argv[3]);
    if (width <= 0 || height <= 0) return 2;
    int fd = open(argv[1], O_RDWR | O_CLOEXEC | O_NOFOLLOW);
    struct stat st;
    if (fd < 0 || fstat(fd, &st) || !S_ISFIFO(st.st_mode) || st.st_uid != getuid()) return 2;
    FILE *input = fdopen(fd, "r");
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 3;
    struct wl_registry *registry = wl_display_get_registry(display);
    const struct wl_registry_listener listener = {global, removed};
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !manager || outputs != 1) return 3;
    struct zwlr_virtual_pointer_v1 *pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer_with_output(manager, NULL, output);
    char line[128];
    while (fgets(line, sizeof(line), input)) {
        struct timespec ts;
        clock_gettime(CLOCK_MONOTONIC, &ts);
        uint32_t now = (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
        int x, y;
        if (sscanf(line, "move %d %d", &x, &y) == 2 && x >= 0 && y >= 0 && x < width && y < height)
            zwlr_virtual_pointer_v1_motion_absolute(pointer, now, x, y, width, height);
        else if (!strcmp(line, "down\n"))
            zwlr_virtual_pointer_v1_button(pointer, now, 0x110, WL_POINTER_BUTTON_STATE_PRESSED);
        else if (!strcmp(line, "up\n"))
            zwlr_virtual_pointer_v1_button(pointer, now, 0x110, WL_POINTER_BUTTON_STATE_RELEASED);
        else if (!strcmp(line, "quit\n")) break;
        else return 4;
        zwlr_virtual_pointer_v1_frame(pointer);
        if (wl_display_roundtrip(display) < 0) return 3;
    }
    zwlr_virtual_pointer_v1_button(pointer, 0, 0x110, WL_POINTER_BUTTON_STATE_RELEASED);
    zwlr_virtual_pointer_v1_frame(pointer);
    zwlr_virtual_pointer_v1_destroy(pointer);
    wl_display_roundtrip(display);
    wl_display_disconnect(display);
    fclose(input);
    return 0;
}
