use bevy_app::prelude::App;

pub trait SendEvent<T> {
    fn dispatch(&mut self, event: T);
}

pub trait EventSet {
    fn add_events(app: &mut App);
}

pub trait AddEventSet {
    fn add_event_set<E: EventSet>(&mut self) -> &mut Self;
}

impl AddEventSet for App {
    fn add_event_set<E: EventSet>(&mut self) -> &mut Self {
        E::add_events(self);
        self
    }
}

macro_rules! event_set {
    ($vis:vis $name:ident {$($event:ident),+}) => {
        #[allow(non_snake_case)]
        #[derive(bevy_ecs::system::SystemParam)]
        $vis struct $name<'w> {
            $(
                $event: bevy_ecs::prelude::EventWriter<'w, $event>
            ),+
        }

        impl<'w> EventSet for $name<'w> {
            fn add_events(app: &mut App) {
                $(
                    app.add_event::<$event>();
                )+
            }
        }

        $(
            impl<'w> SendEvent<$event> for $name<'w> {
                fn dispatch(&mut self, event: $event) {
                    self.$event.send(event);
                }
            }
        )+
    };
}

pub(crate) use event_set;

pub mod prelude {
    pub(crate) use super::event_set;
    pub(crate) use super::AddEventSet;
    pub(crate) use super::EventSet;
    pub(crate) use super::SendEvent;
}
