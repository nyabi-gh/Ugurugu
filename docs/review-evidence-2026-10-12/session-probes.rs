
#[cfg(test)]
mod review_audit {
    use super::*;
    use ugu_core::ops::{Affine, Op};

    #[test]
    fn rejected_transform_is_dropped_when_changing_tools() {
        let mut doc = Document::new([8, 8]);
        let LayerKind::Paint(paint) = &mut doc.layers[0].kind else { unreachable!() };
        paint.ops = vec![Op::Crop { offset: [0, 0], size: [8, 8] }; limits::OPERATIONS];
        doc.validate().unwrap();
        let mut session = Session::new(doc.clone(), true);
        session.select_all();
        session.begin_transform().unwrap();
        assert!(session.set_transform(Affine::translation(1.0, 0.0)));
        assert!(session.is_dirty());
        // The snapshot used by recovery silently loses the transform as well.
        assert_eq!(session.settled_document().unwrap(), doc);
        let outcome = session.apply_transform();
        println!("R05 apply={outcome:?} pending={} dirty={}",
            session.pending().is_some(), session.is_dirty());
        assert!(outcome.is_err());
        assert!(session.pending().is_none());
        assert!(!session.is_dirty());
        session.begin_transform().unwrap();
        session.set_transform(Affine::translation(1.0, 0.0));
        session.set_tool(Tool::Eraser);
        assert_eq!(session.document(), &doc);
        assert!(!session.is_dirty());
    }
}
