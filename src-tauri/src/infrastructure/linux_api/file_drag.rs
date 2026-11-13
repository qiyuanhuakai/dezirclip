use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc};

pub fn start(window: &tauri::WebviewWindow, paths: Vec<String>, done: impl FnOnce(Result<(), String>) + 'static) {
    let widget = match window.gtk_window() { Ok(window) => window, Err(error) => { done(Err(error.to_string())); return; } };
    if !widget.is_visible() { done(Err("WindowUnavailable".into())); return; }
    let held = widget.window().and_then(|native| {
        native.display().default_seat().and_then(|seat| seat.pointer())
            .map(|pointer| native.device_position(&pointer).3.contains(gtk::gdk::ModifierType::BUTTON1_MASK))
    }).unwrap_or(false);
    if !held { done(Ok(())); return; }
    let uris = paths.iter().map(|path| tauri::Url::from_file_path(path).map(|url| url.to_string()))
        .collect::<Result<Vec<_>, _>>();
    let Ok(uris) = uris else { done(Err("FileDragPathInvalid".into())); return; };
    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    let handlers = Rc::new(RefCell::new(Vec::new()));
    let completion = Rc::new(RefCell::new(Some(done)));
    handlers.borrow_mut().push(widget.connect_drag_data_get(move |_, _, selection, _, _| {
        let values: Vec<&str> = uris.iter().map(String::as_str).collect();
        selection.set_uris(&values);
    }));
    let ids = handlers.clone();
    let callback = completion.clone();
    handlers.borrow_mut().push(widget.connect_drag_end(move |widget, _| {
        for id in ids.borrow_mut().drain(..) { widget.disconnect(id); }
        if let Some(done) = callback.borrow_mut().take() { done(Ok(())); }
    }));
    let ids = handlers.clone();
    let callback = completion.clone();
    handlers.borrow_mut().push(widget.connect_destroy(move |widget| {
        for id in ids.borrow_mut().drain(..) { widget.disconnect(id); }
        if let Some(done) = callback.borrow_mut().take() { done(Err("FileDragWindowClosed".into())); }
    }));
    if widget.drag_begin_with_coordinates(&targets, gtk::gdk::DragAction::COPY, 1, None, -1, -1).is_none() {
        for id in handlers.borrow_mut().drain(..) { widget.disconnect(id); }
        if let Some(done) = completion.borrow_mut().take() { done(Err("FileDragUnavailable".into())); }
    }
}
