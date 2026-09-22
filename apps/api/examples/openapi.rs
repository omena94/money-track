use utoipa::OpenApi;
fn main() {
    println!("{}", money_api::ApiDoc::openapi().to_pretty_json().unwrap());
}
