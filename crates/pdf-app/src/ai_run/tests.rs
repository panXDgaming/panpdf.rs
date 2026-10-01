use super::{Cannot, Run};

fn after_steps(revisions: &[u64]) -> Run {
    let mut run = Run::default();
    let mut now = revisions.first().copied().map(|first| first - 1);
    for revision in revisions {
        run.before_a_step(now);
        run.landed(Some(*revision));
        now = Some(*revision);
    }
    run
}

#[test]
fn a_run_that_changed_the_document_can_take_every_step_back() {
    let run = after_steps(&[4, 5, 6]);
    assert_eq!(run.steps(), 3);
    assert_eq!(run.may_take_back(Some(6)), Ok(3));
}

#[test]
fn a_run_that_changed_nothing_has_nothing_to_take_back() {
    let mut run = Run::default();
    run.before_a_step(Some(9));
    assert_eq!(run.may_take_back(Some(9)), Err(Cannot::NothingOfTheirs));
    assert_eq!(run.steps(), 0);
}

#[test]
fn a_persons_edit_after_the_run_refuses_taking_it_back() {
    let run = after_steps(&[4, 5]);
    assert_eq!(
        run.may_take_back(Some(6)),
        Err(Cannot::PersonEdited),
        "the document moved on past the assistant's last step"
    );
    assert_eq!(
        run.may_take_back(Some(5)),
        Ok(2),
        "known answer: untouched, it is allowed"
    );
}

#[test]
fn a_persons_edit_between_two_steps_refuses_taking_the_run_back() {
    let mut run = Run::default();
    run.before_a_step(Some(3));
    run.landed(Some(4));
    run.before_a_step(Some(5));
    run.landed(Some(6));
    assert_eq!(
        run.may_take_back(Some(6)),
        Err(Cannot::PersonEdited),
        "revision 5 was the person's: undoing the run would undo it too"
    );
}

#[test]
fn the_run_begins_at_its_first_step_whatever_the_person_did_before_it() {
    let mut run = Run::default();
    run.before_a_step(Some(2));
    run.before_a_step(Some(7));
    run.landed(Some(8));
    assert_eq!(
        run.may_take_back(Some(8)),
        Ok(1),
        "an edit made while the assistant was still thinking lies under the run, not in it"
    );
}

#[test]
fn a_step_taken_back_can_be_put_back_until_something_new_is_done() {
    let mut run = after_steps(&[4, 5]);
    run.took_one_back(Some(6));
    assert_eq!((run.steps(), run.steps_taken_back()), (1, 1));
    assert_eq!(run.may_put_back(Some(6)), Ok(1));
    run.put_one_back(Some(7));
    assert_eq!((run.steps(), run.steps_taken_back()), (2, 0));
    assert_eq!(run.may_put_back(Some(7)), Err(Cannot::NothingOfTheirs));

    run.took_one_back(Some(8));
    run.before_a_step(Some(8));
    run.landed(Some(9));
    assert_eq!(
        run.may_put_back(Some(9)),
        Err(Cannot::NothingOfTheirs),
        "a new step ends what could be put back"
    );
}

#[test]
fn a_run_taken_back_to_nothing_and_edited_by_the_person_stays_refused() {
    let mut run = after_steps(&[4]);
    run.took_one_back(Some(5));
    assert_eq!(run.may_take_back(Some(5)), Err(Cannot::NothingOfTheirs));
    run.landed(Some(7));
    run.took_one_back(Some(8));
    assert_eq!(run.may_put_back(Some(9)), Err(Cannot::PersonEdited));
}

#[test]
fn a_run_with_no_step_yet_has_nothing_to_take_back_whatever_the_person_did() {
    let run = Run::default();
    assert_eq!(run.may_take_back(Some(12)), Err(Cannot::NothingOfTheirs));
    assert_eq!(run.may_put_back(Some(12)), Err(Cannot::NothingOfTheirs));
}
