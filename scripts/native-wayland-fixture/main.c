/* Controlled protocol failures only. This fixture never owns a clipboard. */
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wayland-server.h>
#include "ext-data-control-server.h"

static void destroy_resource(struct wl_client *client, struct wl_resource *resource) {
    (void)client;
    wl_resource_destroy(resource);
}
static const struct wl_seat_interface seat_impl = {
    .release = destroy_resource,
};
static void bind_seat(struct wl_client *client, void *data, uint32_t version, uint32_t id) {
    (void)data;
    struct wl_resource *seat = wl_resource_create(client, &wl_seat_interface, version < 2 ? version : 2, id);
    wl_resource_set_implementation(seat, &seat_impl, NULL, NULL);
    wl_seat_send_capabilities(seat, 0);
    if (wl_resource_get_version(seat) >= 2) wl_seat_send_name(seat, "fixture-seat");
}
static void ignore_selection(struct wl_client *client, struct wl_resource *device, struct wl_resource *source) {
    (void)client; (void)device; (void)source;
}
static const struct ext_data_control_device_v1_interface device_impl = {
    .set_selection = ignore_selection,
    .destroy = destroy_resource,
    .set_primary_selection = ignore_selection,
};
static void get_device(struct wl_client *client, struct wl_resource *manager, uint32_t id, struct wl_resource *seat) {
    (void)manager; (void)seat;
    struct wl_resource *device = wl_resource_create(client, &ext_data_control_device_v1_interface, 1, id);
    wl_resource_set_implementation(device, &device_impl, NULL, NULL);
    ext_data_control_device_v1_send_finished(device);
}
static void ignore_offer(struct wl_client *client, struct wl_resource *source, const char *mime) {
    (void)client; (void)source; (void)mime;
}
static const struct ext_data_control_source_v1_interface source_impl = {
    .offer = ignore_offer,
    .destroy = destroy_resource,
};
static void create_source(struct wl_client *client, struct wl_resource *manager, uint32_t id) {
    (void)manager;
    struct wl_resource *source = wl_resource_create(client, &ext_data_control_source_v1_interface, 1, id);
    wl_resource_set_implementation(source, &source_impl, NULL, NULL);
}
static const struct ext_data_control_manager_v1_interface manager_impl = {
    .create_data_source = create_source,
    .get_data_device = get_device,
    .destroy = destroy_resource,
};
static void bind_manager(struct wl_client *client, void *data, uint32_t version, uint32_t id) {
    (void)data; (void)version;
    struct wl_resource *manager = wl_resource_create(client, &ext_data_control_manager_v1_interface, 1, id);
    wl_resource_set_implementation(manager, &manager_impl, NULL, NULL);
}
static int terminate(int signal_number, void *data) {
    (void)signal_number;
    wl_display_terminate(data);
    return 0;
}
int main(int argc, char **argv) {
    if (argc != 2 || (strcmp(argv[1], "protocol") && strcmp(argv[1], "seat") && strcmp(argv[1], "permission"))) return 2;
    struct wl_display *display = wl_display_create();
    if (!display) return 3;
    if (strcmp(argv[1], "protocol")) wl_global_create(display, &ext_data_control_manager_v1_interface, 1, NULL, bind_manager);
    if (strcmp(argv[1], "seat")) wl_global_create(display, &wl_seat_interface, 2, NULL, bind_seat);
    if (wl_display_add_socket(display, "wayland-negative")) return 4;
    struct wl_event_source *signal_source = wl_event_loop_add_signal(wl_display_get_event_loop(display), SIGTERM, terminate, display);
    if (!signal_source) return 5;
    puts("ready");
    fflush(stdout);
    wl_display_run(display);
    wl_event_source_remove(signal_source);
    wl_display_destroy_clients(display);
    wl_display_destroy(display);
    return 0;
}
