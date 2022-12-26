use solana_program::{
    entrypoint::ProgramResult,
    pubkey::Pubkey,
    msg,
    account_info::{next_account_info, AccountInfo},
    system_instruction,
    program_error::ProgramError,
    sysvar::{rent::Rent, Sysvar, rent::ID as RENT_PROGRAM_ID},
    native_token::LAMPORTS_PER_SOL,
    system_program::ID as SYSTEM_PROGRAM_ID,
    program::{invoke_signed},
    borsh::try_from_slice_unchecked,
    program_pack::{IsInitialized},
};
use std::convert::TryInto;
use crate::instruction::MovieInstruction;
use crate::state::{ MovieAccountState, MovieCommentCounter, MovieComment };
use borsh::BorshSerialize;
use crate::error::ReviewError;
use spl_associated_token_account::get_associated_token_address;
use spl_token::{instruction::{initialize_mint, mint_to}, ID as TOKEN_PROGRAM_ID};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8]
  ) -> ProgramResult {
    let instruction = MovieInstruction::unpack(instruction_data)?;
    match instruction {
      MovieInstruction::AddMovieReview { title, rating, description } => {
        add_movie_review(program_id, accounts, title, rating, description)
      },
      MovieInstruction::UpdateMovieReview { title, rating, description } => {
        update_movie_review(program_id, accounts, title, rating, description)
      },
      MovieInstruction::AddComment { comment } => {
        add_comment(program_id, accounts, comment)
      },
      MovieInstruction::InitializeMint => initialize_token_mint(program_id, accounts)
    }
}

pub fn add_movie_review(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    title: String,
    rating: u8,
    description: String
) -> ProgramResult {
    msg!("Adding movie review...");
    msg!("Title: {}", title);
    msg!("Rating: {}", rating);
    msg!("Description: {}", description);

    let account_info_iter = &mut accounts.iter();

    let initializer = next_account_info(account_info_iter)?;
    let pda_account = next_account_info(account_info_iter)?;
    let pda_counter = next_account_info(account_info_iter)?;
    let token_mint = next_account_info(account_info_iter)?;
    let mint_auth = next_account_info(account_info_iter)?;
    let user_ata = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let token_program = next_account_info(account_info_iter)?;

    if !initializer.is_signer {
        msg!("Missing required signature");
        return Err(ProgramError::MissingRequiredSignature)
    }

    let (pda, bump_seed) = Pubkey::find_program_address(&[initializer.key.as_ref(), title.as_bytes().as_ref(),], program_id);
    if pda != *pda_account.key {
        msg!("Invalid seeds for PDA");
        return Err(ProgramError::InvalidArgument)
    }

    let (counter_pda, counter_bump) = Pubkey::find_program_address(
        &[pda.as_ref(), "comment".as_ref()],
        program_id
    );

    if counter_pda != *pda_counter.key {
        msg!("Invalid seeds for counter PDA");
        return Err(ProgramError::InvalidArgument);
    }

    if rating > 5 || rating < 1 {
        msg!("Rating cannot be higher than 5");
        return Err(ReviewError::InvalidRating.into())
    }

    msg!("Deriving mint authority");
    let (mint_pda, _mint_bump) = Pubkey::find_program_address(&[b"token_mint"], program_id);
    let (mint_auth_pda, mint_auth_bump) = Pubkey::find_program_address(&[b"token_auth"], program_id);

    if *token_mint.key != mint_pda {
        msg!("Incorrect token mint");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *mint_auth.key != mint_auth_pda {
        msg!("Mint passed in and mint derived do not match");
        return Err(ReviewError::InvalidPDA.into());
    }

    if *user_ata.key != get_associated_token_address(initializer.key, token_mint.key) {
        msg!("Incorrect token mint");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *token_program.key != TOKEN_PROGRAM_ID {
        msg!("Incorrect token program");
        return Err(ReviewError::IncorrectAccountError.into());
    }


    let account_len: usize = 1000;

    if MovieAccountState::get_account_size(title.clone(), description.clone()) > account_len {
        msg!("Data length is larger than 1000 bytes");
        return Err(ReviewError::InvalidDataLength.into())
    }

    let rent = Rent::get()?;
    let rent_lamports = rent.minimum_balance(account_len);

    invoke_signed(
        &system_instruction::create_account(
        initializer.key,
        pda_account.key,
        rent_lamports,
        account_len.try_into().unwrap(),
        program_id,
        ),
        &[initializer.clone(), pda_account.clone(), system_program.clone()],
        &[&[initializer.key.as_ref(), title.as_bytes().as_ref(), &[bump_seed]]],
    )?;

    msg!("PDA created: {}", pda);

    msg!("unpacking state account");
    let mut account_data = try_from_slice_unchecked::<MovieAccountState>(&pda_account.data.borrow()).unwrap();
    msg!("borrowed account data");

    msg!("checking if movie account is already initialized");
    if account_data.is_initialized() {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    account_data.discriminator = MovieAccountState::DISCRIMINATOR.to_string();
    account_data.review = *initializer.key;
    account_data.title = title;
    account_data.rating = rating;
    account_data.description = description;
    account_data.is_initialized = true;

    msg!("serializing account");
    account_data.serialize(&mut &mut pda_account.data.borrow_mut()[..])?;
    msg!("state account serialized");


    msg!("Creating comment counter");
    let counter_rent_lamports = rent.minimum_balance(MovieCommentCounter::SIZE);

    // Derives address and validates that correct seeds were passed in
    let (counter, counter_bump) = Pubkey::find_program_address(&[pda.as_ref(), "comment".as_ref()], program_id);
    if counter != *pda_counter.key {
        msg!("Invalid seeds for PDA");
        return Err(ProgramError::InvalidArgument);
    }

    // Creating the comment counter account 
    invoke_signed(
        &system_instruction::create_account(
        initializer.key, // rent payer
        pda_counter.key, // address who we're creating the account for
        counter_rent_lamports, // amount of rent to put into account
        MovieCommentCounter::SIZE.try_into().unwrap(), // size of account
        program_id,
        ),
        // List of accounts that will be read from/written to
        &[initializer.clone(), pda_counter.clone(), system_program.clone()],
        // Seeds for the PDA
        &[&[pda.as_ref(), "comment".as_ref(), &[counter_bump]]],
    )?;
    msg!("Comment counter created");

    // Deserialize the newly created counter account
    let mut counter_data = try_from_slice_unchecked::<MovieCommentCounter>(&pda_counter.data.borrow()).unwrap();

    msg!("Checking if counter account is already initialized");
    if counter_data.is_initialized() {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    counter_data.discriminator = MovieCommentCounter::DISCRIMINATOR.to_string();
    counter_data.counter = 0;
    counter_data.is_initialized = true;
    msg!("Comment count: {}", counter_data.counter);
    counter_data.serialize(&mut &mut pda_counter.data.borrow_mut()[..])?;
    msg!("Comment counter initialized");


    msg!("Minting 10 token to User associated token account");
    invoke_signed(
        &spl_token::instruction::mint_to(
            token_program.key,
            token_mint.key,
            user_ata.key,
            mint_auth.key,
            &[],
            10 * LAMPORTS_PER_SOL
        )?, // ? unwraps and returns the error if there is one
        // Account infos
        &[token_mint.clone(), user_ata.clone(), mint_auth.clone()],
        // Seeds
        &[&[b"token_auth", &[mint_auth_bump]]]
    )?;


    Ok(())
}

pub fn update_movie_review(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _title: String,
    rating: u8,
    description: String
) -> ProgramResult {    
    msg!("Updating movie review...");

    let account_info_iter = &mut accounts.iter();

    let initializer = next_account_info(account_info_iter)?;
    let pda_account = next_account_info(account_info_iter)?;

    if pda_account.owner != program_id {
      return Err(ProgramError::IllegalOwner)
    }

    if !initializer.is_signer {
        msg!("Missing required signature");
        return Err(ProgramError::MissingRequiredSignature)
    }

    msg!("unpacking state account");
    let mut account_data = try_from_slice_unchecked::<MovieAccountState>(&pda_account.data.borrow()).unwrap();
    msg!("review title: {}", account_data.title);

    let (pda, _bump_seed) = Pubkey::find_program_address(&[initializer.key.as_ref(), account_data.title.as_bytes().as_ref(),], program_id);
    if pda != *pda_account.key {
        msg!("Invalid seeds for PDA");
        return Err(ReviewError::InvalidPDA.into())
    }

    msg!("checking if movie account is initialized");
    if !account_data.is_initialized() {
        msg!("Account is not initialized");
        return Err(ReviewError::UninitializedAccount.into());
    }

    if rating > 5 || rating < 1 {
        msg!("Invalid Rating");
        return Err(ReviewError::InvalidRating.into())
    }

    let update_len: usize = 1 + 1 + (4 + description.len()) + account_data.title.len();
    if update_len > 1000 {
        msg!("Data length is larger than 1000 bytes");
        return Err(ReviewError::InvalidDataLength.into())
    }

    msg!("Review before update:");
    msg!("Title: {}", account_data.title);
    msg!("Rating: {}", account_data.rating);
    msg!("Description: {}", account_data.description);

    account_data.rating = rating;
    account_data.description = description;

    msg!("Review after update:");
    msg!("Title: {}", account_data.title);
    msg!("Rating: {}", account_data.rating);
    msg!("Description: {}", account_data.description);

    msg!("serializing account");
    account_data.serialize(&mut &mut pda_account.data.borrow_mut()[..])?;
    msg!("state account serialized");

    Ok(())
}

pub fn add_comment(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    comment: String
) -> ProgramResult {
    msg!("Adding comment...");
    msg!("Comment: {}", comment);

    let account_info_iter = &mut accounts.iter();

    let commenter = next_account_info(account_info_iter)?;
    let pda_review = next_account_info(account_info_iter)?;
    let pda_counter = next_account_info(account_info_iter)?;
    let pda_comment = next_account_info(account_info_iter)?;
    let token_mint = next_account_info(account_info_iter)?;
    let mint_auth = next_account_info(account_info_iter)?;
    let user_ata = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let token_program = next_account_info(account_info_iter)?;

    let mut counter_data = try_from_slice_unchecked::<MovieCommentCounter>(&pda_counter.data.borrow()).unwrap();

    let account_len = MovieComment::get_account_size(comment.clone());

    let rent = Rent::get()?;
    let rent_lamports = rent.minimum_balance(account_len);

    let (pda, bump_seed) = Pubkey::find_program_address(
        &[pda_review.key.as_ref(), counter_data.counter.to_be_bytes().as_ref()],
        program_id
    );

    if pda != *pda_comment.key {
        msg!("Invalid seeds for PDA");
        return Err(ReviewError::InvalidPDA.into());
    }

    msg!("Deriving mint authority");
    let (mint_pda, _mint_bump) = Pubkey::find_program_address(&[b"token_mint"], program_id);
    let (mint_auth_pda, mint_auth_bump) = Pubkey::find_program_address(&[b"token_auth"], program_id);

    if *token_mint.key != mint_pda {
        msg!("Incorrect token mint");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *mint_auth.key != mint_auth_pda {
        msg!("Mint passed in and mint derived do not match");
        return Err(ReviewError::InvalidPDA.into());
    }

    if *user_ata.key != get_associated_token_address(commenter.key, token_mint.key) {
        msg!("Incorrect token mint");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *token_program.key != TOKEN_PROGRAM_ID {
        msg!("Incorrect token program");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    invoke_signed(
        &system_instruction::create_account(
            commenter.key,
            pda_comment.key,
            rent_lamports,
            account_len.try_into().unwrap(),
            program_id
        ),
        &[commenter.clone(), pda_comment.clone(), system_program.clone()],
        &[&[pda_review.key.as_ref(), counter_data.counter.to_be_bytes().as_ref(), &[bump_seed]]]
    )?;
    msg!("Created comment account");

    let mut comment_data = try_from_slice_unchecked::<MovieComment>(&pda_comment.data.borrow()).unwrap();

    msg!("Checking if comment account is already initialized");
    if comment_data.is_initialized {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    comment_data.discriminator = MovieComment::DISCRIMINATOR.to_string();
    comment_data.review = *pda_review.key;
    comment_data.commenter = *commenter.key;
    comment_data.comment = comment;
    comment_data.is_initialized = true;
    comment_data.serialize(&mut & mut pda_comment.data.borrow_mut()[..])?;

    msg!("Comment count: {}", counter_data.counter);
    counter_data.counter += 1;
    counter_data.serialize(&mut &mut pda_counter.data.borrow_mut()[..])?;


    msg!("Minting 5 tokens to User associated token account");
    invoke_signed(
        // Instruction
        &spl_token::instruction::mint_to(
            token_program.key,
            token_mint.key,
            user_ata.key,
            mint_auth.key,
            &[],
            5 * LAMPORTS_PER_SOL
        )?,
        // Account Infos
        &[token_mint.clone(), user_ata.clone(), mint_auth.clone()],
        // Seeds
        &[&[b"token_auth", &[mint_auth_bump]]]
    )?;

    Ok(())
}

pub fn initialize_token_mint(
    program_id: &Pubkey,
    accounts: &[AccountInfo]
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // The order of the following accounts is not arbitrary!
    // They are sent by the client in this order

    // Sender of transaction
    let initializer = next_account_info(account_info_iter)?;
    // Token mint PDA - derived on the clinet
    let token_mint = next_account_info(account_info_iter)?;
    // Token mint authority (should be me)
    let mint_auth = next_account_info(account_info_iter)?;
    // System program to create a new account
    let system_program = next_account_info(account_info_iter)?;
    // Solana Token program address
    let token_program = next_account_info(account_info_iter)?;
    // System account to calculate the rent
    let sysvar_rent = next_account_info(account_info_iter)?;

    // Derive the mint PDA again to validate
    let (mint_pda, mint_bump) = Pubkey::find_program_address(&[b"token_mint"], program_id);

    // Derive the mint authority to validate
    let (mint_auth_pda, _mint_auth_bump) = Pubkey::find_program_address(&[b"token_auth"], program_id);

    msg!("Token mint: {:?}", mint_pda);
    msg!("Mint authority: {:?}", mint_auth_pda);

    // Validate the important accounts passed in
    if mint_pda != *token_mint.key {
        msg!("Incorrect token mint account");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *token_program.key != TOKEN_PROGRAM_ID {
        msg!("Incorrect token program");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *mint_auth.key != mint_auth_pda {
        msg!("Incorrect mint auth account");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *system_program.key != SYSTEM_PROGRAM_ID {
        msg!("Incorrect system program");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    if *sysvar_rent.key != RENT_PROGRAM_ID {
        msg!("Incorrect rent program");
        return Err(ReviewError::IncorrectAccountError.into());
    }

    let rent = Rent::get()?;
    // The size of a mint account is 82! Remember this!
    let rent_lamports = rent.minimum_balance(82);

    // Create the token mint PDA
    invoke_signed(
        &system_instruction::create_account(
            initializer.key,
            token_mint.key,
            rent_lamports,
            82, // size of token mint account
            token_program.key
        ),
        // Accounts we're reading from or writing to
        &[
          initializer.clone(),
          token_mint.clone(),
          system_program.clone(),
        ],
        // Seeds for our token mint account
        &[&[b"token_mint", &[mint_bump]]]
    )?;

    msg!("Created token mint account");

    // Initialize mint account
    invoke_signed(
        &initialize_mint(
            token_program.key,
            token_mint.key,
            mint_auth.key,
            Option::None, // Freeze authority - we don't want anyone to be able to freeze
            9, // Number of decimals
        )?,
        // Which accounts we're reading from or writing to
        &[token_mint.clone(), sysvar_rent.clone(), mint_auth.clone()],
        // Seeds for our token mint PDA
        &[&[b"token_mint", &[mint_bump]]]
    )?;

    msg!("Initialized token mint");

    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::*,
        solana_program::{
            instruction::{AccountMeta, Instruction},
            system_program::ID as SYSTEM_PROGRAM_ID,
        },
        solana_program_test::*,
        solana_sdk::{
            signature::Signer,
            transaction::Transaction,
            sysvar::rent::ID as SYSVAR_RENT_ID,
        },
        spl_associated_token_account::{
            get_associated_token_address,
            instruction::create_associated_token_account
        },
        spl_token::ID as TOKEN_PROGRAM_ID
    };

    fn create_init_mint_ix(payer: Pubkey, program_id: Pubkey) -> (Pubkey, Pubkey, Instruction) {
        // Derive PDA for token mint authority
        let (mint, _bump_seed) = Pubkey::find_program_address(&[b"token_mint"], &program_id);
        let (mint_auth, _bump_seed) = Pubkey::find_program_address(&[b"token_auth"], &program_id);

        let init_mint_ix = Instruction {
            program_id: program_id,
            accounts: vec![
                AccountMeta::new_readonly(payer, true),
                AccountMeta::new(mint, false),
                AccountMeta::new(mint_auth, false),
                AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(SYSVAR_RENT_ID, false),
            ],
            data: vec![3]
        };
        (mint, mint_auth, init_mint_ix)
    }

    #[tokio::test]
    async fn test_initialize_mint_instruction() {
        let program_id = Pubkey::new_unique();
        let (mut banks_client, payer, recent_blockhash) = ProgramTest::new(
            "pda_local",
            program_id,
            processor!(process_instruction)
        )
        .start()
        .await;

        // Call helper function
        let (_mint, _mint_auth, init_mint_ix) = create_init_mint_ix(payer.pubkey(), program_id);

        // Create transaction object with instructions, accounts, and input data
        let mut transaction = Transaction::new_with_payer(
            &[init_mint_ix,],
            Some(&payer.pubkey())
        );
        transaction.sign(&[&payer], recent_blockhash);

        // Process transaction and compare the result
        assert_matches!(banks_client.process_transaction(transaction).await, Ok(_));
    }

    #[tokio::test]
    async fn test_add_movie_review_instruction() {
        let program_id = Pubkey::new_unique();
        let (mut banks_client, payer, recent_blockhash) = ProgramTest::new(
            "pda_local",
            program_id,
            processor!(process_instruction)
        )
        .start()
        .await;

        // Call helper function
        let (mint, mint_auth, init_mint_ix) = create_init_mint_ix(payer.pubkey(), program_id);

        // Create review PDA
        let title: String = "Captain America".to_owned();
        const RATING: u8 = 3;
        let review: String = "Liked the move".to_owned();
        let (review_pda, _bump_seed) = Pubkey::find_program_address(&[payer.pubkey().as_ref(), title.as_bytes()], &program_id);

        // Create comment PDA
        let (comment_pda, _bump_seed) = Pubkey::find_program_address(&[review_pda.as_ref(), b"comment"], &program_id);

        // Create user associated token account of token mint
        let init_ata_ix: Instruction = create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint
        );

        let user_ata: Pubkey = get_associated_token_address(&payer.pubkey(), &mint);

        // Concat data to single buffer
        let mut data_vec = vec![0];
        data_vec.append(
            &mut (TryInto::<u32>::try_into(title.len()).unwrap().to_le_bytes())
              .try_into()
              .unwrap()
        );

        data_vec.append(&mut title.into_bytes());
        data_vec.push(RATING);
        data_vec.append(
            &mut (TryInto::<u32>::try_into(review.len())
                  .unwrap()
                  .to_le_bytes())
            .try_into()
            .unwrap()
        );
        data_vec.append(&mut review.into_bytes());

        // Create transaction object with instructions, accounts, and input data
        let mut transaction = Transaction::new_with_payer(
            &[
                init_mint_ix,
                init_ata_ix,
                Instruction {
                    program_id: program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(payer.pubkey(), true),
                        AccountMeta::new(review_pda, false),
                        AccountMeta::new(comment_pda, false),
                        AccountMeta::new(mint, false),
                        AccountMeta::new_readonly(mint_auth, false),
                        AccountMeta::new(user_ata, false),
                        AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
                        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                    ],
                    data: data_vec,
                }
            ],
            Some(&payer.pubkey())
        );
        transaction.sign(&[&payer], recent_blockhash);

        // Process transaction and compare the result
        assert_matches!(banks_client.process_transaction(transaction).await, Ok(_));
    }
}

