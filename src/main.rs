use point_cloud_lib::App;

fn main() {
    pollster::block_on(App::run());
}
