// use bytes::Bytes;
// use rqx_core::streaming::*;

// #[cfg(test)]
// mod tests {

//     /// Feed `chunks` through a chunker of `size` and collect what it yields.
//     fn rechunk(size: usize, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
//         let mut chunker = ByteChunker::new(Some(size));
//         let mut out = Vec::new();
//         for chunk in chunks {
//             chunker.feed(Bytes::copy_from_slice(chunk));
//             while let Some(piece) = chunker.next_full() {
//                 out.push(piece.to_vec());
//             }
//         }
//         if let Some(rest) = chunker.flush() {
//             out.push(rest.to_vec());
//         }
//         out
//     }

//     #[test]
//     fn byte_chunker_yields_exact_pieces_and_a_remainder() {
//         let body: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
//         for size in [1usize, 3, 7, 64, 333, 999, 1000, 1001] {
//             for cut in [1usize, 5, 128, 512, 1000] {
//                 let chunks: Vec<&[u8]> = body.chunks(cut).collect();
//                 let pieces = rechunk(size, &chunks);
//                 assert!(
//                     pieces[..pieces.len() - 1].iter().all(|p| p.len() == size),
//                     "size {size} cut {cut}"
//                 );
//                 assert!(!pieces.last().unwrap().is_empty() && pieces.last().unwrap().len() <= size);
//                 assert_eq!(pieces.concat(), body, "size {size} cut {cut}");
//             }
//         }
//     }

//     #[test]
//     fn chunkers_without_a_size_pass_chunks_through() {
//         let mut bytes = ByteChunker::new(None);
//         bytes.feed(Bytes::from_static(b"abc"));
//         assert_eq!(bytes.next_full().as_deref(), Some(&b"abc"[..]));
//         assert_eq!(bytes.next_full(), None);
//         assert_eq!(bytes.flush(), None);
//         let mut text = TextChunker::new(None);
//         text.feed("héllo");
//         assert_eq!(text.next_full().as_deref(), Some("héllo"));
//         assert_eq!(text.next_full(), None);
//         assert_eq!(text.flush(), None);
//     }

//     #[test]
//     fn byte_chunker_empty_body_yields_nothing() {
//         assert!(rechunk(16, &[]).is_empty());
//         assert!(rechunk(16, &[b""]).is_empty());
//     }

//     #[test]
//     fn text_chunker_counts_characters() {
//         let mut chunker = TextChunker::new(Some(2));
//         chunker.feed("aé€🙂b");
//         assert_eq!(chunker.next_full().as_deref(), Some("aé"));
//         assert_eq!(chunker.next_full().as_deref(), Some("€🙂"));
//         assert_eq!(chunker.next_full(), None);
//         assert_eq!(chunker.flush().as_deref(), Some("b"));
//         assert_eq!(chunker.flush(), None);
//     }

//     #[test]
//     fn text_chunker_yields_a_piece_of_exactly_size_without_waiting_for_more() {
//         // A live stream that sends exactly `size` characters and pauses must not stall.
//         let mut chunker = TextChunker::new(Some(3));
//         chunker.feed("a€🙂");
//         assert_eq!(chunker.next_full().as_deref(), Some("a€🙂"));
//         assert_eq!(chunker.next_full(), None);
//         chunker.feed("bc");
//         assert_eq!(chunker.next_full(), None);
//         chunker.feed("d");
//         assert_eq!(chunker.next_full().as_deref(), Some("bcd"));
//         assert_eq!(chunker.flush(), None);
//     }

//     #[test]
//     fn split_lines_matches_splitlines() {
//         assert_eq!(LineDecoder::split_lines("a\nb"), ["a", "b"]);
//         assert_eq!(LineDecoder::split_lines("a\n"), ["a"]); // no trailing empty after a terminator
//         assert_eq!(LineDecoder::split_lines("a\n\n"), ["a", ""]);
//         assert!(LineDecoder::split_lines("").is_empty());
//         assert_eq!(LineDecoder::split_lines("a\r\nb"), ["a", "b"]); // CRLF is a single break
//         assert_eq!(LineDecoder::split_lines("a\rb"), ["a", "b"]); // lone CR is a break
//     }

//     #[test]
//     fn feed_emits_complete_lines() {
//         let mut d = LineDecoder::default();
//         assert_eq!(d.feed("a\nb\nc\n"), ["a", "b", "c"]);
//     }

//     #[test]
//     fn feed_buffers_partial_line_across_chunks() {
//         let mut d = LineDecoder::default();
//         assert!(d.feed("ab").is_empty()); // unterminated — buffered, nothing yet
//         assert_eq!(d.feed("cd\n"), ["abcd"]); // completed by the next chunk
//     }

//     #[test]
//     fn feed_reassembles_crlf_split_across_chunks() {
//         // The case we can't force over a socket: "\r\n" straddles the boundary.
//         // The trailing "\r" must be deferred, not emitted as a lone-CR line.
//         let mut d = LineDecoder::default();
//         assert!(d.feed("a\r").is_empty()); // trailing CR deferred
//         assert_eq!(d.feed("\nb\n"), ["a", "b"]); // no spurious empty line
//     }

//     #[test]
//     fn feed_treats_lone_cr_as_terminator() {
//         let mut d = LineDecoder::default();
//         assert_eq!(d.feed("a\rb\r"), ["a"]); // "b" deferred (its own trailing CR)
//         assert_eq!(d.flush(), Some("b".to_string()));
//     }

//     #[test]
//     fn flush_emits_final_unterminated_line() {
//         let mut d = LineDecoder::default();
//         assert!(d.feed("last line").is_empty());
//         assert_eq!(d.flush(), Some("last line".to_string()));
//         assert_eq!(d.flush(), None); // nothing left
//     }
// }
