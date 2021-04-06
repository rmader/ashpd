use std::{convert::TryFrom, sync::Arc};

use ashpd::zbus;
use ashpd::{
    desktop::location::{AsyncLocationProxy, CreateSessionOptions, Location, SessionStartOptions},
    BasicResponse, HandleToken, Response, WindowIdentifier,
};
use futures::{lock::Mutex, FutureExt};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use adw::subclass::prelude::*;
    use gtk::CompositeTemplate;

    use super::*;

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/com/belmoussaoui/ashpd/demo/location.ui")]
    pub struct LocationPage {}

    #[glib::object_subclass]
    impl ObjectSubclass for LocationPage {
        const NAME: &'static str = "LocationPage";
        type Type = super::LocationPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.install_action("location.locate", None, move |page, _action, _target| {
                page.locate();
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }
    impl ObjectImpl for LocationPage {}
    impl WidgetImpl for LocationPage {}
    impl BinImpl for LocationPage {}
}

glib::wrapper! {
    pub struct LocationPage(ObjectSubclass<imp::LocationPage>) @extends gtk::Widget, adw::Bin;
}

impl LocationPage {
    pub fn new() -> Self {
        glib::Object::new(&[]).expect("Failed to create a LocationPage")
    }

    pub fn locate(&self) {
        let self_ = imp::LocationPage::from_instance(self);

        let ctx = glib::MainContext::default();
        ctx.spawn_local(async move {
            let location = locate(WindowIdentifier::default()).await;
            println!("{:#?}", location);
        });
    }
}

pub async fn locate(window_identifier: WindowIdentifier) -> zbus::Result<Response<Location>> {
    let connection = zbus::azync::Connection::new_session().await?;
    let proxy = AsyncLocationProxy::new(&connection)?;
    let session = proxy
        .create_session(
            CreateSessionOptions::default()
                .session_handle_token(HandleToken::try_from("sometokenstuff").unwrap()),
        )
        .await?;

    let request = proxy
        .start(&session, window_identifier, SessionStartOptions::default())
        .await?;

    let (request_sender, request_receiver) = futures::channel::oneshot::channel();
    let request_sender = Arc::new(Mutex::new(Some(request_sender)));
    let request_id = request
        .connect_response(move |response: Response<BasicResponse>| {
            let s = request_sender.clone();
            async move {
                if let Some(m) = s.lock().await.take() {
                    let _ = m.send(response);
                }
                Ok(())
            }
            .boxed()
        })
        .await?;

    while request.next_signal().await?.is_some() {}
    if let Response::Err(err) = request_receiver.await.unwrap() {
        return Ok(Response::Err(err));
    }

    let (location_sender, location_receiver) = futures::channel::oneshot::channel();
    let location_sender = Arc::new(Mutex::new(Some(location_sender)));
    let signal_id = proxy
        .connect_location_updated(move |location| {
            let s = location_sender.clone();
            async move {
                if let Some(m) = s.lock().await.take() {
                    let _ = m.send(location);
                }
                Ok(())
            }
            .boxed()
        })
        .await?;

    while proxy.next_signal().await?.is_some() {}
    proxy.disconnect_signal(signal_id).await?;
    request.disconnect_signal(request_id).await?;
    session.close().await?;

    let location = location_receiver.await.unwrap();
    Ok(Response::Ok(location))
}