use point_cloud_lib::App;

fn main() {
    let future = App {
        canvas_id: None,
        url: None,
    }
    .run();

    pollster::block_on(future);
}
