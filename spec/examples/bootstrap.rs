use spec::data_dictionary::DataDictionary;

fn main() {
    println!("boostrap");

    let mut ddic = DataDictionary::new();
    ddic.activate();
    ddic.dump();
}
