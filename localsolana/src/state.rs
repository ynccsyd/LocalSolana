use borsh::{BorshSerialize, BorshDeserialize};
use solana_program::{
    program_pack::{IsInitialized, Sealed},
    pubkey::Pubkey,
};

#[derive(BorshSerialize, BorshDeserialize)]
pub struct MovieAccountState {
    pub discriminator: String,
    pub is_initialized: bool,
    pub review: Pubkey,
    pub rating: u8,
    pub title: String,
    pub description: String,
}

// Struct for recording how many comments total
#[derive(BorshSerialize, BorshDeserialize)]
pub struct MovieCommentCounter {
    pub discriminator: String,
    pub is_initialized: bool,
    pub counter: u64,
}

// Struct for storing individual comments
#[derive(BorshSerialize, BorshDeserialize)]
pub struct MovieComment {
    pub discriminator: String,
    pub is_initialized: bool,
    pub review: Pubkey,
    pub commenter: Pubkey,
    pub comment: String,
    pub count: u64,
}

// Use Sealed if account size is not dynamic
impl Sealed for MovieAccountState {}
impl Sealed for MovieCommentCounter {}

impl IsInitialized for MovieAccountState {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl IsInitialized for MovieCommentCounter {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl IsInitialized for MovieComment {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl MovieAccountState {
    pub const DISCRIMINATOR: &'static str = "review";

    pub fn get_account_size(title: String, description: String) -> usize {
        // 4 bytes to store the size of the subsequent dynamic data string
        return (4 + MovieAccountState::DISCRIMINATOR.len())
            + 1 // for is_initialized
            + 1 // for rating
            + (4 + title.len()) // 4 to store subsequent dynamic data string
            + (4 + description.len()); // 4 to store subsequent dynamic data string
    }
}

impl  MovieComment {
    pub const DISCRIMINATOR: &'static str = "comment";
    pub fn get_account_size(comment: String) -> usize {
        return (4 + MovieComment::DISCRIMINATOR.len())
            + 1 // for is_initialized
            + 32 // for movie review pubkey
            + 32 // for commenter pubkey
            + (4 + comment.len()) // 4 to store subsequent dynamic data string
            + 8; // for count (u64)
    }
}

impl MovieCommentCounter {
    pub const DISCRIMINATOR: &'static str = "counter";
    pub const SIZE: usize = (4 + MovieCommentCounter::DISCRIMINATOR.len()) + 1 + 8;
}
