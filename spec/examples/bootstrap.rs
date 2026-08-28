use spec::data_dictionary::DataDictonary;

fn main() {
    println!("boostrap");

    let mut ddic = DataDictonary::new();
    ddic.activate();
    ddic.dump();
}
