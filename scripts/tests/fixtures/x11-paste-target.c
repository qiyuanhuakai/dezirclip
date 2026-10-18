#include <X11/Xlib.h>
#include <X11/keysym.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <time.h>
#include <unistd.h>

int main(void) {
    const char text[] = "DezirClip\r\n中文🙂\n";
    Display *display = XOpenDisplay(NULL);
    if (!display) return 2;
    Window root = DefaultRootWindow(display);
    Window owner = XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0);
    Window target = XCreateSimpleWindow(display, root, 0, 0, 320, 160, 0, 0, 0);
    Atom clipboard = XInternAtom(display, "CLIPBOARD", False);
    Atom utf8 = XInternAtom(display, "UTF8_STRING", False);
    Atom result = XInternAtom(display, "DEZIRCLIP_TEST_RESULT", False);
    XSelectInput(display, target, KeyPressMask | PropertyChangeMask);
    XMapWindow(display, target);
    XSetSelectionOwner(display, clipboard, owner, CurrentTime);
    XSetInputFocus(display, target, RevertToParent, CurrentTime);
    XSync(display, False);
    if (XGetSelectionOwner(display, clipboard) != owner) return 3;
    puts("READY");
    fflush(stdout);
    time_t deadline = time(NULL) + 15;
    int requested = 0;
    while (time(NULL) < deadline) {
        if (!XPending(display)) {
            fd_set reads;
            FD_ZERO(&reads);
            FD_SET(ConnectionNumber(display), &reads);
            struct timeval wait = {0, 100000};
            select(ConnectionNumber(display) + 1, &reads, NULL, NULL, &wait);
            continue;
        }
        XEvent event;
        XNextEvent(display, &event);
        if (event.type == KeyPress && (event.xkey.state & ControlMask)
            && XLookupKeysym(&event.xkey, 0) == XK_v) {
            requested = 1;
            XConvertSelection(display, clipboard, utf8, result, target, CurrentTime);
            XFlush(display);
        } else if (event.type == SelectionRequest) {
            XSelectionRequestEvent request = event.xselectionrequest;
            XEvent response = {0};
            response.xselection.type = SelectionNotify;
            response.xselection.display = display;
            response.xselection.requestor = request.requestor;
            response.xselection.selection = request.selection;
            response.xselection.target = request.target;
            response.xselection.time = request.time;
            response.xselection.property = None;
            if (request.target == utf8 && request.property != None) {
                XChangeProperty(display, request.requestor, request.property,
                    utf8, 8, PropModeReplace, (const unsigned char *)text, strlen(text));
                response.xselection.property = request.property;
            }
            XSendEvent(display, request.requestor, False, 0, &response);
            XFlush(display);
        } else if (event.type == SelectionNotify && requested) {
            Atom type;
            int format;
            unsigned long count, remaining;
            unsigned char *value = NULL;
            if (event.xselection.property == None) return 4;
            int status = XGetWindowProperty(display, target, result, 0, 4096,
                True, utf8, &type, &format, &count, &remaining, &value);
            int matches = status == Success && type == utf8 && format == 8
                && remaining == 0 && count == strlen(text)
                && memcmp(value, text, count) == 0;
            if (value) XFree(value);
            XCloseDisplay(display);
            if (!matches) return 5;
            puts("PASS Ctrl+V clipboard roundtrip");
            return 0;
        }
    }
    fprintf(stderr, "No matching Ctrl+V clipboard roundtrip before timeout\n");
    XCloseDisplay(display);
    return 6;
}
